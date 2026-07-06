-- Espelha server/065_orders_additional.sql.
-- Valor adicional/acréscimo do pedido (PDV) que SOMA ao total.
ALTER TABLE orders ADD COLUMN additional_amount REAL NOT NULL DEFAULT 0;
