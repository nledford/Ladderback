# The hub

One of your machines can be the **hub**. The others pair with it, and its **Dashboard** shows each of them: whether it is online, its plans, and its latest operations. Each machine still runs its own Ladderback, with its own plans, schedules and web UI, and keeps backing up when it can't reach the hub. The hub also decides which one machine maintains each repository, as [Maintenance with a hub](hub-maintenance.md) says.

Hubs aren't in a release yet: the MVP's release brings them ([decision 122](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/122-hub-out-of-releases-until-m8.md)), and this page describes them as that release has them. Turning a machine into the hub is a few lines of `config.toml`; pairing a machine takes its terminal and the hub's web UI.

## What crosses the network

A paired machine tells its hub its hostname, its time zone and its version of Ladderback; when each of its operations started and ended and how it came out, with a failure's error code but never its arguments; its plans' names, with how and when each last ran and when it runs next; and the progress of the operation it runs. For each repository it uses, it sends the repository's ID, whether it asks to maintain it, and whether it holds another machine's recent snapshots. The hub answers with its approval and its leases.

No password, key, file name or path crosses: the hub never asks a machine for its secrets, and keeps its own. Both sides prove who they are on every connection, and every message after that is signed ([decision 120](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/120-agent-hub-connection-security.md) has the details).

## Turning a machine into the hub

Choose a machine that is on whenever the others are: it keeps their reports and grants their leases. Any installed machine becomes the hub in place, with its plans, snapshots and data folder as they are. Moving a machine into the [Docker image](docker.md) to make it the hub isn't supported yet.

1. **Turn sign-in on** ([Sign-in](remote-access.md#sign-in)) if you will use the hub's web UI from another computer: from there, it makes a pairing token or approves a machine only with sign-in on.
2. **Add the `[serve]` table** to the hub's `config.toml`, in its settings folder ([Install](install.md#where-ladderback-keeps-your-files) lists it), with the address it listens on and the names the other machines and your browser reach it by:

   ```toml
   [serve]
   hub = true
   listen = "0.0.0.0:9898"
   allowed-hosts = ["nas.example.com"]
   ```

3. **Restart Ladderback**: **Restart Ladderback** in Settings, or `ladderback install-service` again. The service reads the table each time it starts.

[Settings in config.toml](remote-access.md#settings-in-configtoml) says how the table and the flags `--hub`, `--listen` and `--allowed-host` of `ladderback serve` go together, and what turning hub mode off does once machines are paired.

On its first start as the hub, Ladderback makes the hub key, its recovery key and the hub's TLS key in its data folder, and its log names the hub key by its fingerprint, such as `7K3Q-M9XD-2RTV-8HFA-QW5N`. That fingerprint is how you tell the hub apart from anything else that answers at its address.

### The hub's address

The hub serves its own TLS. On the address it listens on, it answers `https://` with a certificate of its own, which names `localhost`, the listening address and each allowed host, such as `https://nas.example.com:9898`. It closes a plain `http://` connection from any other computer; on its own computer `http://127.0.0.1:9898` still works. Your browser doesn't know the certificate, so it warns the first time you open the hub's `https://` address, and bookmarks and MCP connect addresses for it change to that address. The machines need no certificate authority: each pairing token pins the hub's certificate.

A reverse proxy in front of the hub is optional; [A hub behind a reverse proxy](remote-access.md#a-hub-behind-a-reverse-proxy) says what one needs.

### Keeping the hub's keys

The files `hub-key` and `hub-tls-key` in the hub's data folder are what every paired machine knows the hub by. Anyone who copies them can pose as the hub, and without them no paired machine accepts the hub. Copy both somewhere only you can read, as you would a password. A plan of Ladderback's that backs up the data folder leaves them out, with the recovery seed and the machine key, so that no repository another machine can read holds them.

A hub whose `hub-key` is gone while machines are paired with it makes no new one, and a hub whose `hub-tls-key` is gone serves only its own computer. Either way its log says the file is missing and to restore it from your copy; its own backups go on meanwhile.

## Pairing a machine

Pairing takes two steps: a pairing token from the hub, given to `ladderback pair` on the machine; then your approval on the hub, once you have compared the machine's key. Each token pairs one machine, once.

### Making a pairing token

In the hub's web UI, open **Settings**, then its **Machines** tab. Under **Make a pairing token**:

- **Address machines will use**: the hub's address as the machine reaches it, such as `https://nas.example.com:9898`. The field warns when an address is loopback, which another machine can't reach, or plain `http://`, which a machine refuses.
- **Certificate the token trusts**: choose **The hub's own certificate** when the hub serves the address itself. **A certificate authority's certificate**, chosen at first, is for a proxy in front of the hub whose certificate a certificate authority signed.
- **Life in minutes**: from 5 to 1440, and 15 unless you change it.
- To confirm, the admin password, or with sign-in off, the **Hub key fingerprint**, which the tab shows beside **Copy the fingerprint**.

Press **Make a pairing token**. The dialog **Copy the pairing token** shows the token once, with the hub key's fingerprint in full and when the token lapses; **Copy the token** copies it. Closing the dialog cancels the token if no machine has used it, so pair the machine first, or copy the token somewhere safe until you do. **Cancel this token** ends it at once.

At the hub's terminal, `ladderback pairing-token --address https://nas.example.com:9898` makes a token too, and prints the hub key's fingerprint before it; `--ttl` sets its life in minutes, `--list` lists the unused tokens and `--revoke` revokes one by its token ID. It runs only while the hub's Ladderback is stopped.

### Pairing on the machine

On the machine, run this as yourself, not under `sudo`:

```sh
ladderback pair
```

Paste the token at `Paste the pairing token:`; it isn't shown as you paste. `pair` then says what it read: the hub's address and the hub key's fingerprint. Compare the fingerprint with the one on the hub's screen, and answer `y` to `Pair? [y/N]` only when they match: another fingerprint means something else answers at that address. Nothing is sent before you answer.

Once the hub has taken the pairing, `pair` says `Paired. The hub holds this machine pending until someone approves it there.` and prints this machine's key fingerprint. Keep it on screen: it is what you compare on the hub. Ladderback can run on the machine meanwhile; it finds the pairing within 30 seconds.

- **The hostname.** The machine pairs under its computer's name, or the one `--hostname` gives. The hub refuses a name another machine holds, naming that machine, and keeps the name a machine first paired under.
- **The address.** An address after `pair`, as in `ladderback pair https://192.0.2.10:9898`, replaces the token's when the machine reaches the hub another way. `pair` refuses a plain `http://` address on another computer, which would show the token to the network.
- **In a script.** `pair` reads the token as standard input too; `--yes` then answers its question, once you have compared the fingerprint.

### Approving it on the hub

The hub's **Machines** tab lists the machine under **Waiting for approval**, with its hostname, agent ID, key fingerprint and how long it has waited. A name that looks like another machine's, as `lаptop` with a Cyrillic `а` looks like `laptop`, is noted on its row. Press **Approve** on the machine's row: the review shows its key fingerprint in full. Compare it with the one `ladderback pair` printed on that machine, confirm as for a token, and approve it.

The machine moves to **Approved machines**. It learns that it is approved the next time it tries the hub, at most 5 minutes later, and then shows on the Dashboard. A machine nobody approves within 7 days lapses: the hub drops it and frees its hostname, and the machine pairs again with a new token. **Reject** turns away a machine you don't recognize.

`ladderback hub-status` on the machine checks its link to the hub: it says the hub holds it waiting for approval, or `Connected. The hub approved this machine.` At the hub's terminal, while its Ladderback is stopped, `ladderback pairing-token --pending` lists the machines waiting, and `ladderback pairing-token --approve <hostname>` shows one's agent ID and key fingerprint and asks before it approves it.

## The Dashboard

The hub's **Dashboard** shows each approved machine. Its start page and its **Machines** tab lead there by the link **See the approved machines' latest operations**.

- **Online now**, or **Last connected** and when. A running machine holds one connection to its hub and sends something on it at least every 30 seconds. The hub closes a connection that sent nothing for 90 seconds, so a machine that sleeps, shuts down or loses the network shows when it last connected within 90 seconds. After the hub restarts, each machine shows when it last connected until it connects again.
- **Plans of** the machine, as it last reported them: how and when each last ran, and when it runs next.
- **Latest operations of** the machine, newest first: when each started and ended, its result, and a failure's error code. A machine keeps its reports while it can't reach the hub and sends them in order once it can. When the hub let go of a machine's oldest reports to stay within its limit, the page says so.
- The progress of the operation the machine runs.

Times read in your browser's time zone, with the machine's own beside them when the two differ. **Pause live updates** holds the page still until you press it again. The hub's own plans and operations stay on its own pages, as on any machine.

## Removing a machine

On the hub's **Machines** tab, **Remove** on an approved machine's row opens a review with its key fingerprint, and **Reject** does the same for a waiting one. Neither asks for a password, since each only takes authority away. Once confirmed, the machine can't sync with the hub, or pair with it again under its machine ID; it keeps backing up, and maintains no repository. Its hostname is free for another machine.

The removed machine's start page says **Your hub removed this computer** once it next connects. To pair it again, press **Take a new machine ID** there, restart Ladderback, and pair it with a new token, which the hub approves as a new machine. A new ID turns maintenance off on every repository the machine maintained: turn it back on for each one it should maintain. The snapshots made under the old ID stay under it until you adopt them into a plan.

## Upgrading before 1.0

Until 1.0.0, a release may change how machines and the hub talk to each other, as [the README's warning](https://github.com/nledford/Ladderback/blob/main/README.md) says. Such a release raises the minor version, as 0.6 to 0.7, and its entry in [CHANGELOG.md](https://github.com/nledford/Ladderback/blob/main/CHANGELOG.md) lists the change under **Breaking**, with the steps to upgrade, which may be pairing each machine again. Read it before you upgrade the hub or any machine. A machine that can't speak with its hub keeps backing up on its own meanwhile.

## Known limits

- **No alert when a machine goes offline yet.** The Dashboard shows when each machine last connected, but nothing tells you when one stops ([#570](https://github.com/nledford/Ladderback/issues/570)).
- **Losing the hub's machine.** Machines keep backing up, and within 7 days their leases run out, after which they maintain nothing ([Maintenance with a hub](hub-maintenance.md)). There is no way yet to recover the hub ([#1466](https://github.com/nledford/Ladderback/issues/1466)), nor to pair its machines with a new one: `ladderback pair` on a machine that pinned one hub refuses another hub's token, with `An unexpected error occurred` ([#1637](https://github.com/nledford/Ladderback/issues/1637)). Keep your copy of `hub-key` and `hub-tls-key`.
- **A machine can't leave its hub.** A machine that has paired maintains a repository only under its hub's lease, even after the hub removed it. A supported way to forget the hub comes with [#1637](https://github.com/nledford/Ladderback/issues/1637).
- **Pairing tokens at the hub's terminal.** Listing and revoking the unused tokens work only while the hub's Ladderback is stopped. A token made in the web UI is canceled by closing its dialog.
