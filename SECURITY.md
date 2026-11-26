# Security policy

## Reporting a vulnerability

Report it privately with GitHub's [private vulnerability reporting](https://github.com/nledford/Ladderback/security/advisories/new), never in a public issue, discussion or pull request. Say what an attacker can do, what they need to do it, and the steps that show it.

Ladderback is maintained by one person in their own time. You will get an answer, but not on a fixed schedule; a fix is released as soon as it is ready, and the advisory names you unless you ask otherwise.

## What is in scope

Ladderback trusts the person who runs it and every program running as their operating-system user, which includes their data folder, its files and its database ([decision 99](docs/design/decisions/99-who-is-trusted.md)). A finding that needs such a program already running isn't a vulnerability.

In scope is what crosses that line: the network (the web UI and API, a hub and the machines paired with it), other users of the same machine, a repository's storage, and text from outside, such as a snapshot's contents or an imported profile. So is any way a resolved secret reaches an argument list, an environment, a log, a message or the web UI.

## Supported versions

Until 1.0, only the latest release gets security fixes.
