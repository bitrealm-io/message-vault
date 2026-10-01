# Security policy

Message Crate stores people's private message archives, so security reports
matter more here than in most projects. If you have found a vulnerability —
or think you may have — we want to hear about it quickly and quietly.

## Supported versions

Message Crate is a self-hosted product: the desktop app and the server
both run on machines you control. Because fixes are only ever published for
the current release, security updates apply to the **latest release only**.
If you are running an older version and suspect you are affected by a known
issue, upgrade first and re-test before reporting.

## Reporting a vulnerability

Please do not open a public issue, and please do not post details in
discussions or chat. Report suspected vulnerabilities privately to:

**[vault@bitrealm.io](mailto:vault@bitrealm.io)**

Include as much of the following as you can:

- the affected component and version (server, desktop app, web frontend, or
  a specific exporter),
- a description of the behavior you observed and why it is a problem,
- steps to reproduce, ideally minimal,
- any workaround you have found.

Reports are read by the maintainer. There is no bug bounty program, and
there is no PGP key for this address yet — both may change as the project
grows.

## What to expect

- **Acknowledgment within 5 business days**, with an initial assessment.
- **Updates at least every 14 days** while the report is open.
- **A fix, a scheduled fix, or a written explanation within 90 days** of
  acknowledgment for confirmed vulnerabilities. Most fixes land far sooner;
  the window exists for reports that need a coordinated disclosure or a
  careful migration.
- **Credit in the changelog** for reporters who want it. If you prefer to
  stay anonymous, that is fine too.

## Disclosure

By default we follow coordinated disclosure: the report stays private until
a fix is released, and we agree on a disclosure date with the reporter.
Once the fix ships, reporters are encouraged to publish their write-up —
the changelog entry for the fix will credit them if they wish.
