use crate::define_model;

define_model!(
    ContactModel,
    "contacts",
    fields: {
        peer_id: String,
        name: String,
        address: Option<String>,
    },
    create: {
        sql: "INSERT INTO contacts (peer_id, name, address) VALUES (?, ?, ?)",
        params: [peer_id, name, address]
    },
    update: {
        sql: "UPDATE contacts SET peer_id = ?, name = ?, address = ? WHERE id = ?",
        params: [peer_id, name, address]
    }
);
