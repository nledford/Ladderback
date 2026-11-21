-- The fingerprint of the hub's TLS key (technical design §9; decision 120,
-- "The hub key"), recorded when the hub makes the key: the SHA-256 of its
-- certificate's public key (SubjectPublicKeyInfo). One row at most, written
-- once: a hub that later finds the key file gone, or another key in it,
-- refuses to serve it rather than make a replacement. One addition, no data
-- changes. It ships in every build, though only a build with the hub writes
-- to it (decision 122).
CREATE TABLE hub_tls_key (
  id           INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
  fingerprint  BLOB NOT NULL CHECK (length(fingerprint) = 32)
);
