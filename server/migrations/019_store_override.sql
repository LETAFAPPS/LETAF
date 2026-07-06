-- Override de status do estabelecimento (§6: campo da entidade Company).
-- Valores: 'none' (segue horário), 'open' (forçado aberto), 'closed' (forçado fechado).
ALTER TABLE companies ADD COLUMN IF NOT EXISTS store_override TEXT NOT NULL DEFAULT 'none';
