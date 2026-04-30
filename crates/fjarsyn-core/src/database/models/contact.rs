use crate::define_model;

define_model!(
    ContactModel,
    "contacts",
    fields: {
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    },
    create: {
        sql: "INSERT INTO contacts (peer_id, name, address, trusted_public_key) VALUES (?, ?, ?, ?)",
        params: [peer_id, name, address, trusted_public_key]
    },
    update: {
        sql: "UPDATE contacts SET peer_id = ?, name = ?, address = ?, trusted_public_key = ? WHERE id = ?",
        params: [peer_id, name, address, trusted_public_key]
    }
);
