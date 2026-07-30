-- Status de pagamento do pedido (Pago/Não pago). Vendas do PDV nascem
-- pagas (pagamento na finalização) — backfill marca como pagos os
-- pedidos existentes com forma de pagamento registrada. Pedidos do
-- cardápio web nascem não pagos. Espelha server/087_orders_paid.sql.
ALTER TABLE orders ADD COLUMN paid INTEGER NOT NULL DEFAULT 0;
UPDATE orders SET paid = 1 WHERE payment_method IS NOT NULL AND payment_method != '';
