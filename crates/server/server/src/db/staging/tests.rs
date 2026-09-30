use super::*;
use crate::test_support::test_vault;

const A1: i64 = 7;
const A2: i64 = 8;

#[tokio::test]
async fn reset_for_account_leaves_other_accounts() {
    let vault = test_vault().await;
    let mut conn = vault.conn().await;
    for (account, user) in [(A1, "alice"), (A2, "bob")] {
        vault.account_with_id(account, user).await;
        let conversation_id = insert_conversation(
            &mut conn,
            &StagingConversation {
                account_id: account,
                chat_handle_id: 1,
                conversation_type: "individual",
                group_title: None,
                exported_at: None,
                source_file: "t.json",
            },
        )
        .await
        .unwrap();
        insert_messages(
            &mut conn,
            &[StagingMessage {
                conversation_id,
                account_id: account,
                source: "sms",
                guid: Some("g1"),
                timestamp: "2020-01-01T00:00:00Z",
                is_from_me: 0,
                sender_handle_id: None,
                owner_handle_id: None,
                service: None,
                subject: None,
                body: None,
                is_announcement: 0,
                is_reply: 0,
                thread_originator_guid: None,
                thread_originator_part: None,
                num_replies: 0,
                sort_order: 0,
                import_id: None,
            }],
        )
        .await
        .unwrap();
    }

    reset_for_account(&mut conn, A1).await.unwrap();
    assert_eq!(count_staged_conversations(&mut conn, A1).await.unwrap(), 0);
    assert_eq!(count_staged_conversations(&mut conn, A2).await.unwrap(), 1);
    assert_eq!(
        count_staged_messages(&mut conn, A2).await.unwrap(),
        1,
        "the cascade removes only the reset account's messages"
    );
}
