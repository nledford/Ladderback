# The hub

One of your machines can be the **hub**. The others pair with it, and its **Dashboard** shows each of them: whether it is online, its plans, and its latest operations. Each machine still runs its own Ladderback, with its own plans, schedules and web UI, and keeps backing up when it can't reach the hub. The hub also decides which one machine maintains each repository, as [Maintenance with a hub](hub-maintenance.md) says.

The MVP's release is the first with hubs ([decision 122](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/122-hub-out-of-releases-until-m8.md)); an earlier release can't pair, so update every machine first. Turning a machine into the hub is a few lines of `config.toml`; pairing a machine takes its terminal and the hub's web UI.

## What crosses the network

A paired machine tells its hub its hostname, its time zone, its clock and its version of Ladderback, with a fingerprint of its computer's identifier that tells two computers apart but never gives the identifier back; when each of its operations started and ended and how it came out, with a failure's error code but never its arguments; its plans' names, with how and when each last ran and when it runs next; and the progress of the operation it runs. For each repository it uses, it sends the repository's ID, whether it asks to maintain it, whether it holds another machine's recent snapshots, and the highest lease the hub granted it there, with the hub's signature. The hub answers with its approval and its leases.

No password, secret key, file name or path crosses: the hub never asks a machine for its secrets, and keeps its own. The two exchange only the public halves of their keys, at pairing. Both sides prove who they are on every connection, and every message after that is signed ([decision 120](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/120-agent-hub-connection-security.md) has the details).

## Turning a machine into the hub

Choose a machine that is on whenever the others are: it keeps their reports and grants their leases. Any installed machine can become the hub in place.

1. **Turn sign-in on** ([Sign-in](remote-access.md#sign-in)) if you will use the hub's web UI from another computer: from there, it makes a pairing token, approves, rejects or removes a machine, or ends recovery mode only with sign-in on.
2. **Add the `[serve]` table** to the hub's `config.toml`, in its settings folder ([Install](install.md#where-ladderback-keeps-your-files) lists it), with the address it listens on and the names your browser reaches it by:

   ```toml
   [serve]
   hub = true
   listen = "0.0.0.0:9898"
   allowed-hosts = ["nas.example.com"]
   ```

3. **Restart Ladderback**: **Restart Ladderback** in Settings, or `ladderback install-service` again.

[Settings in config.toml](remote-access.md#settings-in-configtoml) says how the table and the flags `--hub`, `--listen` and `--allowed-host` of `ladderback serve` go together, and what turning hub mode off does once machines are paired.

On its first start as the hub, Ladderback makes the hub key, its recovery key and the hub's TLS key in its data folder, and its log names the hub key by its fingerprint, such as `7K3Q-M9XD-2RTV-8HFA-QW5N`. That fingerprint is how you tell the hub apart from anything else that answers at its address.

### The hub's address

The hub serves its own TLS. On the address it listens on, it answers `https://` with a certificate of its own, which names `localhost`, `127.0.0.1` and `::1`, the listening address unless it is `0.0.0.0`, and each allowed host, such as `https://nas.example.com:9898`. It closes a plain `http://` connection from any other computer; on its own computer `http://127.0.0.1:9898` still works. Your browser doesn't know the certificate, so it warns the first time you open the hub's `https://` address; use that address in your bookmarks. The machines need no certificate authority: a pairing token made with the hub's own certificate pins it.

A reverse proxy in front of the hub is optional; [A hub behind a reverse proxy](remote-access.md#a-hub-behind-a-reverse-proxy) says what one needs.

### Keeping the hub's keys

The files `hub-key` and `hub-tls-key` in the hub's data folder are what every paired machine knows the hub by, and `hub-recovery-seed` beside them holds the seed of the hub's recovery key. Anyone who copies them can pose as the hub, and without the first two no paired machine accepts the hub. Copy all three somewhere only you can read, as you would a password. A plan of Ladderback's that backs up the data folder leaves them out, with the machine key, so that no repository another machine can read holds them.

A hub whose `hub-key` is gone while machines are paired with it makes no new one, and a hub whose `hub-tls-key` is gone serves only its own computer. Either way its log says the file is missing and to restore it from your copy; its own backups go on meanwhile.

## Pairing a machine

Pairing takes two steps: a pairing token from the hub, given to `ladderback pair` on the machine; then your approval on the hub, once you have compared the machine's key. Each token pairs one machine, once.

### Making a pairing token

In the hub's web UI, open **Settings**, then its **Machines** tab. Under **Make a pairing token**:

- **Address machines will use**: the hub's address as the machine reaches it, such as `https://nas.example.com:9898`. The field warns when an address is loopback, which another machine can't reach, or plain `http://`, which a machine refuses.
- **Certificate the token trusts**: **The hub's own certificate** when the hub serves the address itself, as it does for its `https://` address; it is chosen at first when you opened the page at that address. **A certificate authority's certificate**, chosen at first when you opened the page through a proxy or over `http://127.0.0.1`, is for a proxy in front of the hub whose certificate a certificate authority signed.
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

The hub's **Machines** tab lists the machine under **Waiting for approval**, with its hostname, its machine ID (the **Agent ID** column), its key fingerprint and how long it has waited. A name that looks like another machine's, as `lаptop` with a Cyrillic `а` looks like `laptop`, is noted on its row. Press **Approve** on the machine's row: the review shows its key fingerprint in full. Compare it with the one `ladderback pair` printed on that machine, confirm as for a token, and approve it.

The machine moves to **Approved machines**, and shows on the Dashboard at once. It learns that it is approved the next time it tries the hub, at most 5 minutes later, and shows **Online now** from then on. A machine nobody approves within 7 days lapses: the hub drops it and frees its hostname, and the machine pairs again with a new token. **Reject** turns away a machine you don't recognize.

`ladderback hub-status` on the machine checks its link to the hub: it says the hub holds it waiting for approval, or `Connected. The hub approved this machine.` At the hub's terminal, while its Ladderback is stopped, `ladderback pairing-token --pending` lists the machines waiting, and `ladderback pairing-token --approve <hostname>` shows one's machine ID and key fingerprint and asks before it approves it.

## The Dashboard

The hub's **Dashboard** shows each approved machine. Its start page and its **Machines** tab lead there by the link **See the approved machines' latest operations**.

- **Online now**, or **Last connected** and when. A running machine holds one connection to its hub and sends something on it at least every 30 seconds. The hub closes a connection that sent nothing for 90 seconds, so a machine that sleeps, shuts down or loses the network shows when it last connected within 90 seconds. After the hub restarts, each machine shows when it last connected until it connects again.
- **Plans of** the machine, as it last reported them: how and when each last ran, and when it runs next.
- **Latest operations of** the machine, newest first: when each started and ended, its result, and a failure's error code. A machine keeps its reports while it can't reach the hub and sends them in order once it can. When the hub let go of a machine's oldest reports to stay within its limit, the page says so.
- The progress of the operation the machine runs.

Times read in your browser's time zone, with the machine's own beside them when the two differ. **Pause live updates** holds the page still until you press it again. The hub's own plans and operations stay on its own pages, as on any machine.

## Removing a machine

On the hub's **Machines** tab, **Remove** on an approved machine's row opens a review with its key fingerprint, and **Reject** does the same for a waiting one. Neither asks for a password, since each only takes authority away. Once confirmed, the machine can't sync with the hub, or pair with it again under its machine ID. It keeps backing up, and maintains nothing once the leases it holds run out, within 7 days. Its hostname is free for another machine.

The removed machine's start page says **Your hub removed this computer** once it next connects. To pair it again, press **Take a new machine ID** there, restart Ladderback, and pair it with a new token, which the hub approves as a new machine. A new ID turns maintenance off on every repository the machine maintained: turn it back on for each one it should maintain. The snapshots made under the old ID stay under it until you adopt them into a plan.

## Leaving a hub

A machine pairs with one hub, and refuses another hub's token while it is paired: `ladderback pair` names both hubs' key fingerprints and stops. When its hub is lost or replaced, or the hub removed it and it should stand alone, leave the hub at the machine:

1. Stop the machine's Ladderback: `unpair` refuses while it runs, since it holds the hub's leases.
2. Run `ladderback unpair`. It names the hub by its address and key fingerprint and asks before it changes anything; with no terminal to ask on, `--yes` answers.
3. Start Ladderback again as you started it before.

The machine drops the leases it held from that hub and forgets the hub's key. It then maintains each repository whose maintenance is set to this machine, as a machine with no hub does, and `ladderback pair` pairs it with any hub's token. The hub isn't told: a hub that still runs lists the machine, and alerts that it is offline, until you remove it on its **Machines** tab, and gives the repositories the machine maintained to no other machine before the leases it held run out. After that, if another of its machines also maintains one of them, two machines do: turn maintenance off on one of them.

## Upgrading before 1.0

Until 1.0.0, a release may change how machines and the hub talk to each other, as [the README's warning](https://github.com/nledford/Ladderback/blob/main/README.md) says. Such a release raises the minor version, as 0.6 to 0.7, and its entry in [CHANGELOG.md](https://github.com/nledford/Ladderback/blob/main/CHANGELOG.md) lists the change under **Breaking**, with the steps to upgrade, which may be pairing each machine again. Read it before you upgrade the hub or any machine.

**From 0.5.0 to 0.6.0,** install 0.6.0 and start it; there are no other steps. On its first start, each machine keeps its machine ID and key, and copies its operation log into the new schema. The old log is kept beside the new one as `ladderback.db.before-baseline`, which you can delete once you no longer want it. A log that can't be copied is left as it was, and Ladderback says why. Snapshots 0.5.0 took carry the tag `ladderback-agent:<id>`, where 0.6.0 writes `ladderback-machine:<id>`: retag them once as the 0.6.0 release notes describe.

## Known limits

- **Losing the hub's machine.** Machines keep backing up, and within 7 days their leases run out, after which they maintain nothing ([Maintenance with a hub](hub-maintenance.md)). There is no way yet to recover the hub ([#1466](https://github.com/nledford/Ladderback/issues/1466)): pair its machines with a new one once each has left the old one ([Leaving a hub](#leaving-a-hub)). Keep your copy of the hub's key files ([Keeping the hub's keys](#keeping-the-hubs-keys)).
- **A removed machine doesn't leave its hub by itself.** It keeps its hub's key and maintains nothing once its leases run out, until you run `ladderback unpair` on it, or take a new machine ID and pair it again ([Leaving a hub](#leaving-a-hub)).
- **Pairing tokens at the hub's terminal.** Listing and revoking the unused tokens work only while the hub's Ladderback is stopped. A token made in the web UI is canceled by closing its dialog.
