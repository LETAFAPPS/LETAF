-- Quantidade de pedidos por página na grade de Pedidos (default 20).
ALTER TABLE companies
    ADD COLUMN orders_per_page INTEGER NOT NULL DEFAULT 20;
