-- Adiciona campo password_hash ao customers para login de clientes finais (web).
-- Clientes criados pelo ERP desktop terão NULL (sem login web).
-- Clientes registrados via web terão hash bcrypt.
ALTER TABLE customers ADD COLUMN IF NOT EXISTS password_hash TEXT;
