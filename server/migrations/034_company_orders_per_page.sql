-- Quantidade de pedidos exibidos por página na grade de Pedidos.
-- Configuração separada de `products_per_page` porque cards de pedidos
-- carregam mais informação por linha (cliente, data, status, total).
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS orders_per_page INTEGER NOT NULL DEFAULT 20;
