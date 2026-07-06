-- Tabela de pedidos feitos por clientes finais via web.
-- AI_RULES.md §6: UUID, company_id, timestamps, synced.
CREATE TABLE IF NOT EXISTS orders (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    customer_id UUID NOT NULL REFERENCES customers(id),
    status TEXT NOT NULL DEFAULT 'pending',
    total NUMERIC(14, 2) NOT NULL DEFAULT 0,
    notes TEXT,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_orders_company ON orders(company_id);
CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(company_id, customer_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(company_id, status);

-- Itens do pedido — snapshot do produto no momento da compra.
CREATE TABLE IF NOT EXISTS order_items (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    order_id UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id),
    product_name TEXT NOT NULL,
    quantity DOUBLE PRECISION NOT NULL,
    unit_price NUMERIC(14, 2) NOT NULL,
    subtotal NUMERIC(14, 2) NOT NULL,
    notes TEXT,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_order_items_order ON order_items(order_id);
