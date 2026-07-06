-- Adiciona flag de ativo/inativo ao produto no PostgreSQL (AI_RULES.md §6).
-- DEFAULT TRUE garante que todos os produtos existentes continuem ativos.
ALTER TABLE products ADD COLUMN IF NOT EXISTS active BOOLEAN NOT NULL DEFAULT TRUE;
