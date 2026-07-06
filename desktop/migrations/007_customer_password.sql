-- Adiciona campo password_hash ao customers para sync com servidor.
ALTER TABLE customers ADD COLUMN password_hash TEXT;
