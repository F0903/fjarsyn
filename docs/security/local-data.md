# Local data

Fjarsyn keeps desktop preferences, the local cryptographic identity, trusted
contacts, and message history on the local machine. These records have
different security requirements and therefore do not share one configuration
document.

## Desktop settings

`settings.json` contains only desktop preferences and secret-free engine
settings such as capture, video, and network values. `fjarsyn-desktop` owns
this document and writes it atomically. It never contains the local signing
key.

## Local identity

`identity.bin` is owned privately by `fjarsyn-engine`. It stores the local peer
identifier and Ed25519 signing key as one indivisible record. The record is
encrypted with Windows Data Protection API (DPAPI) in the current-user scope,
so another Windows user cannot decrypt it merely by copying the file. Fjarsyn
does not use machine-wide DPAPI protection.

The engine loads or creates this identity before starting peer-session
networking. Serialized private-key material does not cross the engine API,
enter desktop settings state, or travel through UI messages. First publication
is an atomic, immutable install, so interrupted or simultaneous first launches
cannot expose a partial identity or return different identities to competing
processes. Once installed, the identity is loaded rather than replaced.
The former mixed `config.json` format is not a compatibility input; once a
protected identity is durable, Fjarsyn removes that obsolete plaintext file.
Ordinary file deletion does not guarantee forensic erasure on every storage
device; this cleanup prevents continued application use of the old document,
but it is not a secure-delete claim.

DPAPI protects data at rest for the Windows user; it does not protect against
malware or another process already running as that user. The containing
per-user application directory and inherited Windows access controls remain
part of the local security boundary. Fjarsyn deliberately relies on those
per-user inherited ACLs instead of replacing them with an application-defined
DACL; the signing key's confidentiality does not rely on file permissions
alone because the stored payload remains DPAPI-protected.

## Contacts and message history

The SQLite database contains trusted contact identifiers and public keys, plus
chat messages and related metadata. Message bodies are currently stored in
plaintext. Fjarsyn therefore does not yet claim encrypted local chat history or
forensic secure deletion. Retention, user deletion, WAL/journal handling, and
the desired confidentiality model remain explicit follow-up work in
[`TODO.md`](../../TODO.md).
