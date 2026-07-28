-- Nome/rótulo comercial do desconto — espelha o servidor. Vazio = sem
-- rótulo. Preenchido pelo super admin; a loja só exibe.
ALTER TABLE subscriptions ADD COLUMN plan_discount_name TEXT NOT NULL DEFAULT '';
