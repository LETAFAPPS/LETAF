-- Snapshot dos adicionais escolhidos no carrinho (Fase 4).
-- Espelha 042_order_item_addons.sql do server (TEXT em SQLite).
ALTER TABLE order_items ADD COLUMN addons_json TEXT;
