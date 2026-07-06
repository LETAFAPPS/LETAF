-- Adiciona tipo de entrega ao pedido (entrega em casa / retirada no local).
-- AI_RULES.md §6: campo com valor default para retrocompatibilidade.
ALTER TABLE orders ADD COLUMN IF NOT EXISTS delivery_type TEXT NOT NULL DEFAULT 'delivery';
