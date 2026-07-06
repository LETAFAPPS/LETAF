-- Campos de contato e endereço detalhado da empresa.
-- Todos nullable: preenchidos progressivamente pelo operador na tela
-- de Configurações. `document` (CPF/CNPJ) é apenas armazenado, sem
-- validação no MVP — reservado para integrações fiscais futuras.
ALTER TABLE companies ADD COLUMN IF NOT EXISTS whatsapp     TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS email        TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS instagram    TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS document     TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS neighborhood TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS zip_code     TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS city         TEXT;
ALTER TABLE companies ADD COLUMN IF NOT EXISTS uf           TEXT;
