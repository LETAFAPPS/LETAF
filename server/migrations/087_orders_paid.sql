-- Status de pagamento do pedido (Pago/Não pago). Vendas do PDV nascem
-- pagas (pagamento na finalização) — backfill marca como pagos os
-- pedidos existentes com forma de pagamento registrada. Pedidos do
-- cardápio web nascem não pagos. Espelha desktop/079_orders_paid.sql.
ALTER TABLE orders ADD COLUMN paid BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE orders SET paid = TRUE WHERE payment_method IS NOT NULL AND payment_method != '';
