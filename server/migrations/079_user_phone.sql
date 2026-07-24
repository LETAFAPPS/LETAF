-- Telefone de contato do operador (proprietário/admin), exibido no painel
-- do super admin. NULL = sem telefone.
ALTER TABLE users ADD COLUMN phone TEXT;
