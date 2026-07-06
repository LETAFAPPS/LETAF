-- Adiciona flag de ativo/inativo ao produto (AI_RULES.md §6).
-- DEFAULT 1 garante que todos os produtos existentes continuem ativos.
ALTER TABLE products ADD COLUMN active INTEGER NOT NULL DEFAULT 1;
