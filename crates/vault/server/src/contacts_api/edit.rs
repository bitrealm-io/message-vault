//! The edits `PATCH /v1/contacts/{id}` makes: rename a contact, and link,
//! swap or unlink its identities. The queries are in `db::contacts` and
//! `db::handles`; this module decides which to run and what to refuse.

use anyhow::Result as AnyResult;
use message_ir::HandleType;
use sqlx::AnyConnection;

use super::{
    AddContactIdentityRequest, RemoveContactIdentityRequest, UpdateContactIdentityRequest,
    UpdateContactRequest,
};
use crate::db::contacts::{self, contact_id_for_handle};
use crate::db::handles::{self, infer_handle_type_from_shape};
use crate::server::ApiError;

/// Handle type from the service the caller named, falling back to the handle's shape.
fn infer_handle_type(raw: &str, service: Option<&str>) -> HandleType {
    let svc = service
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    match svc.as_str() {
        "phone" | "sms" | "imessage" | "whatsapp" => HandleType::Phone,
        "email" => HandleType::Email,
        "" => infer_handle_type_from_shape(raw),
        _ => HandleType::Other,
    }
}

/// Why a contact edit did not happen.
///
/// The two cases answer with different statuses and the difference is not one
/// a message can be read for, so it is carried in the type. It used to be
/// guessed: `mutate_contact` returned `anyhow` and the handler downcast the
/// error, calling it a 400 unless it found a `sqlx::Error` underneath. That
/// made "not a database error" mean "the person's fault", so any other
/// internal failure — one wrapped in `context`, one from a helper — reached
/// the person as a 400 wearing an internal sentence.
#[derive(Debug)]
pub enum ContactEditError {
    /// The request asks for something the vault will not do, and the person
    /// can fix it by changing the request. The sentence is written for them.
    Refused(String),
    /// Something failed that changing the request would not help. The cause
    /// goes to the log, not to the person.
    Failed(anyhow::Error),
}

impl From<sqlx::Error> for ContactEditError {
    fn from(error: sqlx::Error) -> Self {
        Self::Failed(error.into())
    }
}

/// Anything a helper hands back through `?` is a failure, not a refusal: a
/// refusal is raised deliberately, here, as [`ContactEditError::Refused`].
impl From<anyhow::Error> for ContactEditError {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

impl From<ContactEditError> for ApiError {
    fn from(error: ContactEditError) -> Self {
        match error {
            ContactEditError::Refused(message) => Self::validation(message),
            ContactEditError::Failed(cause) => Self::Internal(cause),
        }
    }
}

/// Shorthand for the eight things a contact edit refuses.
macro_rules! refuse {
    ($($arg:tt)*) => {
        return Err(ContactEditError::Refused(format!($($arg)*)))
    };
}

/// The one edit a `PATCH /v1/contacts/{id}` body asks for.
enum ContactEdit<'a> {
    /// Give the contact a name the person typed.
    Rename(&'a str),
    /// Link an identity.
    AddIdentity(&'a AddContactIdentityRequest),
    /// Swap one linked identity for another.
    UpdateIdentity(&'a UpdateContactIdentityRequest),
    /// Unlink an identity.
    RemoveIdentity(&'a RemoveContactIdentityRequest),
}

impl UpdateContactRequest {
    /// The single edit the body asks for.
    ///
    /// # Errors
    ///
    /// Refused when the body sets none of the four fields or more than one.
    fn edit(&self) -> Result<ContactEdit<'_>, ContactEditError> {
        let mut edits = [
            self.name.as_deref().map(ContactEdit::Rename),
            self.add_identity.as_ref().map(ContactEdit::AddIdentity),
            self.update_identity
                .as_ref()
                .map(ContactEdit::UpdateIdentity),
            self.remove_identity
                .as_ref()
                .map(ContactEdit::RemoveIdentity),
        ]
        .into_iter()
        .flatten();
        match (edits.next(), edits.next()) {
            (Some(edit), None) => Ok(edit),
            _ => {
                refuse!(
                    "exactly one of name, add_identity, update_identity, remove_identity is required"
                )
            }
        }
    }
}

/// Apply a contact mutation. Returns false when the contact is missing.
///
/// # Errors
///
/// Returns an error when the mutation is invalid or a database write fails.
pub async fn mutate_contact(
    conn: &mut AnyConnection,
    account_id: i64,
    contact_id: i64,
    body: &UpdateContactRequest,
) -> Result<bool, ContactEditError> {
    let mut editor = ContactEditor {
        conn,
        account_id,
        contact_id,
    };
    if !editor.exists().await? {
        return Ok(false);
    }
    editor.apply(body.edit()?).await
}

/// One contact of one account under edit. Every edit reads and writes the
/// contact's handle links, so the three things they all need live here and
/// the edits are methods.
struct ContactEditor<'a> {
    conn: &'a mut AnyConnection,
    account_id: i64,
    contact_id: i64,
}

impl ContactEditor<'_> {
    /// True when the contact belongs to this account and is not in the trash.
    async fn exists(&mut self) -> AnyResult<bool> {
        contacts::live_contact_exists(&mut *self.conn, self.account_id, self.contact_id).await
    }

    /// Apply the edit. `true` once the contact is as the edit asked, whether
    /// or not anything had to change.
    async fn apply(&mut self, edit: ContactEdit<'_>) -> Result<bool, ContactEditError> {
        match edit {
            ContactEdit::Rename(name) => self.rename(name).await,
            ContactEdit::AddIdentity(add) => self.add_identity(add).await,
            ContactEdit::UpdateIdentity(upd) => self.update_identity(upd).await,
            ContactEdit::RemoveIdentity(rem) => self.remove_identity(rem).await,
        }
    }

    /// Name the contact.
    async fn rename(&mut self, name: &str) -> Result<bool, ContactEditError> {
        let name = name.trim();
        if name.is_empty() {
            refuse!("name must not be empty");
        }
        // Typing a name in the drawer is the most deliberate naming act in
        // the product, so the row stops being the import's and becomes the
        // person's. `contacts::propose_name` is where that rule lives, along
        // with what it means for the import and the address book.
        contacts::propose_name(
            &mut *self.conn,
            self.account_id,
            self.contact_id,
            name,
            contacts::Origin::User,
        )
        .await?;
        Ok(true)
    }

    /// Link an identity, creating its row when the vault has never seen it.
    async fn add_identity(
        &mut self,
        add: &AddContactIdentityRequest,
    ) -> Result<bool, ContactEditError> {
        let raw = add.address.trim();
        if raw.is_empty() {
            refuse!("address must not be empty");
        }
        let handle_id = self.handle_row(raw, add.service.as_deref()).await?;
        if self.claim(handle_id).await? {
            // Already linked: no address-book change.
            return Ok(true);
        }
        // The person attached this identity themselves, so a later address
        // book load leaves it alone.
        contacts::link_handle_to_contact(
            &mut *self.conn,
            self.account_id,
            handle_id,
            self.contact_id,
            contacts::Origin::User,
        )
        .await?;
        self.touched().await
    }

    /// Replace one linked identity with another.
    async fn update_identity(
        &mut self,
        upd: &UpdateContactIdentityRequest,
    ) -> Result<bool, ContactEditError> {
        let prev = upd.previous_address.trim();
        let next = upd.address.trim();
        if prev.is_empty() || next.is_empty() {
            refuse!("previous_address and address must not be empty");
        }
        let service = upd.service.as_deref();
        let Some(old_id) = self.linked_handle(prev, service).await? else {
            refuse!("previous address not found on contact");
        };
        let new_id = self.handle_row(next, service).await?;
        if old_id == new_id {
            // Both lookups keyed the row on the same platform, so the edit
            // names the handle the contact already has: nothing changes.
            return Ok(true);
        }
        if self.claim(new_id).await? {
            // The new handle is already on this contact, so the edit amounts
            // to dropping the previous one.
            self.unlink(old_id).await?;
        } else {
            contacts::relink_handle(
                &mut *self.conn,
                self.account_id,
                self.contact_id,
                old_id,
                new_id,
            )
            .await?;
        }
        self.touched().await
    }

    /// Unlink an identity. The handle row itself stays: messages still cite it.
    async fn remove_identity(
        &mut self,
        rem: &RemoveContactIdentityRequest,
    ) -> Result<bool, ContactEditError> {
        let raw = rem.address.trim();
        if raw.is_empty() {
            refuse!("address must not be empty");
        }
        let Some(handle_id) = self.linked_handle(raw, rem.service.as_deref()).await? else {
            refuse!("identity not found on contact");
        };
        self.unlink(handle_id).await?;
        self.touched().await
    }

    /// Id of the handle row for `raw` that is linked to this contact, if any.
    async fn linked_handle(&mut self, raw: &str, service: Option<&str>) -> AnyResult<Option<i64>> {
        contacts::linked_handle_id(
            &mut *self.conn,
            self.account_id,
            self.contact_id,
            raw,
            service,
        )
        .await
    }

    /// Insert or find the handle row for `raw`, typed by `service`, without
    /// linking it to the account owner: contact-owned handles must never
    /// become owner identities.
    async fn handle_row(&mut self, raw: &str, service: Option<&str>) -> AnyResult<i64> {
        let handle_type = infer_handle_type(raw, service);
        let (id, _) = handles::upsert_handle_row(
            &mut *self.conn,
            self.account_id,
            raw.trim(),
            handle_type,
            service.and_then(message_ir::trimmed),
        )
        .await?;
        Ok(id)
    }

    /// True when the handle is already linked to this contact. A handle
    /// belongs to one contact per account (the primary key on
    /// `contact_handles`), so one linked elsewhere is refused.
    async fn claim(&mut self, handle_id: i64) -> Result<bool, ContactEditError> {
        match contact_id_for_handle(&mut *self.conn, self.account_id, handle_id).await? {
            Some(owner) if owner == self.contact_id => Ok(true),
            Some(_) => refuse!("identity already linked to another contact"),
            None => Ok(false),
        }
    }

    /// Drop the link between the contact and the handle.
    async fn unlink(&mut self, handle_id: i64) -> AnyResult<()> {
        contacts::unlink_handle(&mut *self.conn, self.account_id, self.contact_id, handle_id).await
    }

    /// Bump the contact's updated-at and report success, for edits that
    /// changed the links but not the contact row.
    async fn touched(&mut self) -> Result<bool, ContactEditError> {
        contacts::touch_contact(&mut *self.conn, self.account_id, self.contact_id).await?;
        Ok(true)
    }
}
