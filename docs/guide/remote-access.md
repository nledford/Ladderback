# Sign-in and remote access

## Sign-in

Sign-in is off until you set an admin password, with `ladderback set-password` or in Settings, under "Admin password". Either way it turns sign-in on the next time Ladderback starts (for the service, use **Restart Ladderback** in Settings, or run `ladderback install-service` again), and Settings says so until then. With sign-in off, anyone with an account on the machine can use Ladderback, so set a password on a machine others use.

To change the password, use `ladderback set-password` again, or Settings, which asks for the current one. A new password signs out every browser, the one you changed it in included, and revokes every API token, so programs that use one need a new one.

How long signing in lasts is set in Settings, under "Signing in". **Stay signed in for** goes from **Until I close the browser** to **Until I sign out**; it is 30 days until you change it, and a shorter choice also signs out the browsers that signed in longer ago. On the sign-in page, **Keep me signed in on this device** keeps that browser signed in however long it goes unused; without it, 12 hours unused sign it out. Use it only on a device no one else uses. **Sign out everywhere** signs out every browser, the one you use included, and leaves API tokens working. Signing in survives a restart of Ladderback.

Turning sign-in off and recovering a forgotten password happen only at the terminal, so nobody who reaches the web UI can do either: stop Ladderback, delete the file `admin-password` from its data folder, and start it again with sign-in off. Then set a new password if you want sign-in back.

## Listening beyond loopback

`ladderback serve --listen` takes an address beyond loopback only with at least one `--allowed-host`, the name the browser reaches Ladderback by. With sign-in off it then listens too, as Backrest does, and warns at start and on every page that **anyone who can reach that address can run and change backups**: do that only on a network you trust.

It serves plain HTTP, so with sign-in on put a reverse proxy with TLS in front of it. A hub serves its own TLS instead ([The hub's address](hub.md#the-hubs-address)). [Technical design §10.1](https://github.com/nledford/Ladderback/blob/main/docs/design/security.md#101-web-ui) has the details.

## Settings in config.toml

The address, the host names and hub mode can live in the `[serve]` table of `config.toml` instead, so the service `ladderback install-service` writes, which runs `ladderback serve --service-managed` with no other flag, starts with them:

```toml
[serve]
listen = "0.0.0.0:9898"
allowed-hosts = ["nas.example.com"]
```

Ladderback reads the table when it starts, so restart it after a change. A flag you give replaces its key: `--listen` the address, and any `--allowed-host` the whole list.

`hub = true` in the same table makes the machine the hub in place, with its plans, snapshots and data folder as they are, and `--hub` or `--hub=false` overrides it. The MVP's release is the first with hubs; [The hub](hub.md) says how to set one up and pair machines with it. Once hub mode is on and machines are paired with it, turning it off doesn't take effect: Ladderback stays the hub, so none of them loses it, and says at start which machines to remove first in **Settings → Machines**. Moving a machine into the Docker image to make it the hub isn't supported yet ([Docker](docker.md)).

## A hub behind a reverse proxy

A hub reached through a reverse proxy must turn sign-in on, by setting an admin password ([Sign-in](#sign-in)). With sign-in off, a hub trusts a request that comes from its own computer and names it as the host. A proxy on that computer sends its requests from there, and some name `127.0.0.1` as their host, as nginx does by default, so a request from anywhere looks as if it came from the hub's own computer. Anyone who can reach the proxy could then use Ladderback as you do, and make a pairing token with nothing but the hub key's fingerprint, which isn't secret. With sign-in off, the hub's **Settings → Machines** says so too. [Decision 120](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/120-agent-hub-connection-security.md) has the details.

The proxy itself needs four things, or the browser's sign-in is refused with **Ladderback refused this request because it came from a different site**:

- **The public name in the allowed host names.** Add the name people type, such as `backups.example.com`, to `allowed-hosts` in `[serve]` ([Settings in config.toml](#settings-in-configtoml)), beside the hub's own names, and restart Ladderback.
- **The browser's `Host` passed through.** Ladderback compares a request's `Origin` with its `Host`, so the proxy must forward the `Host` the browser sent, not the hub's own name. Caddy replaces it with the upstream's name when the upstream is `https://`, and nginx sends the upstream's name unless told otherwise.
- **`https://` to the hub.** A hub serves plain HTTP only to its own computer, and closes a plain HTTP connection from any other one, logging that it did. Point the proxy at the hub's `https://` address, and have it trust the hub's certificate rather than skip the check.
- **WebSockets.** The web UI's live updates and each machine's link to the hub are WebSockets. Caddy passes them as it is; nginx needs the two `Upgrade` lines below.

The hub's certificate is its own, not a certificate authority's, and names the hub's host names and addresses ([The hub's address](hub.md#the-hubs-address)). Save it on the proxy's computer for the proxy to trust, and save it again after you change `allowed-hosts` or `listen`, which change the names in it:

```sh
openssl s_client -connect hub.lan.example:9898 -servername hub.lan.example </dev/null | openssl x509 > ladderback-hub.pem
```

With Caddy, for a hub at `hub.lan.example:9898`:

```caddyfile
backups.example.com {
    reverse_proxy https://hub.lan.example:9898 {
        header_up Host {host}
        transport http {
            tls_server_name hub.lan.example
            tls_trust_pool file /etc/caddy/ladderback-hub.pem
        }
    }
}
```

In OPNsense's Caddy plugin, make the header under **Reverse Proxy → Headers** (**Header** `header_up`, **Header Type** `Host`, **Header Value** `{host}`), then pick it under the handler's **HTTP Headers**.

With nginx:

```nginx
location / {
    proxy_pass https://hub.lan.example:9898;
    proxy_set_header Host $http_host;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_ssl_name hub.lan.example;
    proxy_ssl_server_name on;
    proxy_ssl_verify on;
    proxy_ssl_trusted_certificate /etc/nginx/ladderback-hub.pem;
}
```

`$http_host` keeps the port when the proxy listens on one other than 443. To pair a machine through the proxy, make its token for the proxy's address, such as `https://backups.example.com`, with **A certificate authority's certificate** ([Making a pairing token](hub.md#making-a-pairing-token)).
