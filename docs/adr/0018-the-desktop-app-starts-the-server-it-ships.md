# The desktop app starts the server it ships

A person who installs the desktop app has a working Message Crate with no
second install (#970). The installer carries `message-crate-server` and the
built website beside the app. When the app is pointed at its own address,
`127.0.0.1:8080`, it asks that address what it is: a Message Crate that
answers is used as it is; if nothing answers, the app starts the server as a
separate process and stops it when the app closes; if another program holds
the port, the app says so and starts nothing.

## Why

Hosting stays Docker's job. The app's server exists so that one install is
enough to read messages on the computer the app is on, and it runs only while
the app is open: no background service, no start on boot.

It is the same program the Docker image runs, started with `serve --data-dir
--bind --static-dir` and no config file. A Message Crate behaves the same
whichever way it was started: the same first start with the Demo Account, the
same login card, the same Owner Home. The two differ only in what starts the
server and where it listens.

The port is 8080 in both, so there is one address to document and the app's
default address needs no change. That is also why a Message Crate already
answering there is used rather than treated as a clash: a person with Docker
on the same computer gets the Message Crate they already have.

The data is in the operating system's app-data folder, under `data`, and the
app has a button that opens it. A folder under Documents is often synced by
OneDrive or iCloud, which can corrupt a database that is in use.

The server listens on this computer only. A setting in the app opens it to
the network, for a person who wants to read from a phone while the app is
open; it is off by default and says, where it is switched on, that the
connection is plain HTTP. The app still asks `127.0.0.1:8080` what is
running, whichever way the setting is.

## Considered and rejected

**Running the server inside the app's process.** No second program to ship
or to be warned about by the operating system. Rejected because it makes a
second kind of server, built and tested differently from the one in the
Docker image, and because a server crash would take the app down with it.

**A port of its own, or a free port at each start.** Either avoids the clash
with Docker. Rejected because a second fixed port is a second number to
explain, and a changing port breaks bookmarks and the saved login.

**A background service that outlives the app.** It would let a phone reach
the Message Crate at any time. Rejected because that is hosting, which the
Docker image already does, and a service is far harder to install, update and
remove than an app.

## Consequences

The server program is stopped by killing it. Its database survives that, and
the only work a kill interrupts is an import the closing app was running.

If the app crashes, its server keeps running. The next launch finds a Message
Crate answering and uses it; that server then stays until the computer
restarts or the person ends it.

The installer grows by the size of the server program. The server and the app
are always the same version, since they are built together.

The server program is unsigned, like the app (#1019). ffmpeg is not shipped
(#1017); the server does without it when it is missing.
