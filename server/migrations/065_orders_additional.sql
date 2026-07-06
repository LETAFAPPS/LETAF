-- Valor adicional/acréscimo do pedido (PDV) — taxa/ajuste manual que
-- SOMA ao total. Calculado/validado no servidor, nunca vindo do
-- frontend (§11). Espelhado em desktop/065_orders_additional.sql.
ALTER TABLE orders ADD COLUMN additional_amount NUMERIC(14, 2) NOT NULL DEFAULT 0;
