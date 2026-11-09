### Ladderback's own command-line messages. The program prints each after its
### name and a colon (`ladderback: listening on …`); `clap`'s help and usage
### errors are not here and stay in English.

# The daemon is up. Scripts and the UI tests read the address after "listening on".
# $url (text): where the web UI answers, such as http://127.0.0.1:9898.
cli-listening = listening on { $url }

# The daemon is up but the operating system would not say where.
# $detail (text): the operating system's message. It is shown as it came.
cli-listening-unknown = listening (address unknown: { $detail })

# The daemon could not take its address.
# $address (text): the address it was asked to listen on, such as 127.0.0.1:9898.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-listen = cannot listen on { $address }: { $detail }

# The daemon could not start its async runtime.
# $detail (text): the operating system's message. It is shown as it came.
cli-cannot-start-runtime = cannot start the async runtime: { $detail }

# The web server stopped with an error.
# $detail (text): the server's own message. It is shown as it came.
cli-server-error = server error: { $detail }

# Sign-in is off, so the daemon only listens on loopback addresses.
# $address (text): the address it was asked to listen on, such as 192.168.1.5:9898.
# `127.0.0.1` and `::1` are addresses, typed as they are: keep them string literals.
cli-refuse-non-loopback = refusing to listen on { $address }: sign-in is off, so { -brand-name } only listens on loopback addresses ({ "127.0.0.1" }, { "::1" })
