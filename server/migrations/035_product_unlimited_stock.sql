-- Marca produtos como "estoque ilimitado": catálogo público nunca os
-- esconde por falta de estoque, carrinho web não impõe teto e
-- OrderService::create pula a baixa de stock_quantity. Útil para itens
-- preparados sob demanda (pizzas, sucos, sanduíches).
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS unlimited_stock BOOLEAN NOT NULL DEFAULT FALSE;
