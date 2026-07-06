-- Adiciona coluna cancellation_reason ao pedido (AI_RULES.md §6, §11).
-- Preenchida quando status = 'cancelled'; nula caso contrário.
ALTER TABLE orders ADD COLUMN IF NOT EXISTS cancellation_reason TEXT;
