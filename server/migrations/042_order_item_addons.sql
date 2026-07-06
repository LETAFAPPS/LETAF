-- Snapshot dos adicionais escolhidos no carrinho (Fase 4).
--
-- JSON `[{"name": "...", "price": f64}, ...]` no momento do pedido.
-- `unit_price` já vem com a soma dos addons; este campo é só para o
-- operador ver detalhamento no PDV e para auditoria.
ALTER TABLE order_items
    ADD COLUMN IF NOT EXISTS addons_json TEXT;
