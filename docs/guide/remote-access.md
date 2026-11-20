# Sign-in and remote access

## Sign-in

Sign-in is off until you set an admin password, with `ladderback set-password` or in Settings, under "Admin password". Either way it turns sign-in on the next time Ladderback starts (for the service, use **Restart Ladderback** in Settings, or run `ladderback install-service` again), and Settings says so until then. With sign-in off, anyone with an account on the machine can use Ladderback, so set a password on a machine others use.

To change the password, use `ladderback set-password` again, or Settings, which asks for the current one. A new password signs out every browser, the one you changed it in included, and revokes every API token, so programs that use one need a new one.

How long signing in lasts is set in Settings, under "Signing in". **Stay signed in for** goes from **Until I close the browser** to **Until I sign out**; it is 30 days until you change it, and a shorter choice also signs out the browsers that signed in longer ago. On the sign-in page, **Keep me signed in on this device** keeps that browser signed in however long it goes unused; without it, 12 hours unused sign it out. Use it only on a device no one else uses. **Sign out everywhere** signs out every browser, the one you use included, and leaves API tokens working. Signing in survives a restart of Ladderback.

Turning sign-in off and recovering a forgotten password happen only at the terminal, so nobody who reaches the web UI can do either: stop Ladderback, delete the file `admin-password` from its data folder, and start it again with sign-in off. Then set a new password if you want sign-in back.

## Listening beyond loopback

`ladderback serve --listen` takes an address beyond loopback only with at least one `--allowed-host`, the name the browser reaches Ladderback by. With sign-in off it then listens too, as Backrest does, and warns at start and on every page that **anyone who can reach that address can run and change backups**: do that only on a network you trust.

It serves plain HTTP, so with sign-in on put a reverse proxy with TLS in front of it. [Technical design §10.1](https://github.com/nledford/Ladderback/blob/main/docs/design/security.md#101-web-ui) has the details.
