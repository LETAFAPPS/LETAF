-- Recuperação de senha (operadores do desktop): código de uso único,
-- com hash (bcrypt) e expiração. O login é global por e-mail, então o
-- reset também é por e-mail (sem company_id).
CREATE TABLE IF NOT EXISTS password_resets (
    id         UUID PRIMARY KEY,
    email      TEXT NOT NULL,
    code_hash  TEXT NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    used       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Busca do código ativo por e-mail (não usado).
CREATE INDEX IF NOT EXISTS idx_password_resets_email_active
    ON password_resets (email) WHERE used = FALSE;
