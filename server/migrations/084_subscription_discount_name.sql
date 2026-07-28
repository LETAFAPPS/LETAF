-- Nome/rótulo comercial do desconto (ex.: "Fidelidade"), definido pelo
-- super admin. Exibido no card de pagamento da loja. Vazio = sem rótulo.
ALTER TABLE subscriptions
    ADD COLUMN IF NOT EXISTS plan_discount_name TEXT NOT NULL DEFAULT '';
