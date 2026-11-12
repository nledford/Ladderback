//! The service files: a systemd unit and a launchd plist, each a fixed
//! template with the paths quoted by the file's own rules.

use domain::service::ServicePath;

/// The unit's first line. A file at the unit's place without it as a whole
/// line was not written by `install-service`, and is never touched.
pub const SYSTEMD_MARK: &str =
    "# Written by `ladderback install-service`; `ladderback remove-service` removes it.";

/// The plist's third line, after the XML declaration and the doctype; the
/// same rule as [`SYSTEMD_MARK`].
pub const LAUNCHD_MARK: &str =
    "<!-- Written by `ladderback install-service`; `ladderback remove-service` removes it. -->";

/// The systemd user unit that runs `program serve`. Its cache folder is the
/// one systemd makes for it, private to the user, which the agent is pointed
/// at (decision 59).
pub fn systemd_unit(program: &ServicePath) -> String {
    format!(
        "{SYSTEMD_MARK}\n\
         [Unit]\n\
         Description=Ladderback, a backup manager for rustic\n\
         Documentation=https://github.com/nledford/Ladderback\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart=:{} serve\n\
         CacheDirectory=ladderback\n\
         CacheDirectoryMode=0700\n\
         Environment=LADDERBACK_CACHE_DIR=%C/ladderback\n\
         KillMode=mixed\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        systemd_word(program.as_str())
    )
}

/// `text` as one quoted word of a systemd command line, `%` (a specifier)
/// doubled so that it stays text. The `:` before it in `ExecStart=` keeps
/// systemd from expanding a variable in it (systemd 243 and later), and a
/// `ServicePath` holds no quote, backslash or control character, so nothing
/// else in it means anything to systemd.
fn systemd_word(text: &str) -> String {
    format!("\"{}\"", text.replace('%', "%%"))
}

/// The launchd agent labelled `label` that runs `program serve` from login
/// on, again after a crash, and writes its log to `log`.
pub fn launchd_plist(label: &str, program: &ServicePath, log: &ServicePath) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         {LAUNCHD_MARK}\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{}</string>\n\
         \t\t<string>serve</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         \t<key>KeepAlive</key>\n\
         \t<dict>\n\
         \t\t<key>SuccessfulExit</key>\n\
         \t\t<false/>\n\
         \t</dict>\n\
         \t<key>StandardErrorPath</key>\n\
         \t<string>{}</string>\n\
         </dict>\n\
         </plist>\n",
        xml_text(label),
        xml_text(program.as_str()),
        xml_text(log.as_str())
    )
}

/// `text` as XML character data.
fn xml_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(c),
        }
    }
    escaped
}
