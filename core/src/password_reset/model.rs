use chrono::NaiveDateTime;
use uuid::Uuid;

/// Pedido de redefinição de senha (código de uso único, com hash e
/// expiração). Ver [`crate::password_reset::service`].
#[derive(Debug, Clone)]
pub struct PasswordReset {
    pub id: Uuid,
    /// E-mail do operador (login é global por e-mail).
    pub email: String,
    /// Hash bcrypt do código de 6 dígitos — nunca guardamos em claro.
    pub code_hash: String,
    pub expires_at: NaiveDateTime,
    pub used: bool,
    pub created_at: NaiveDateTime,
}
