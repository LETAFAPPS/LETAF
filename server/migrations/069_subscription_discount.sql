-- Desconto comercial (R$/mês) por estabelecimento, definido pelo super
-- admin no cadastro. Abatido do valor cobrado por ciclo (× meses). O
-- billing usa `terms()`, que já aplica o desconto. `0` = sem desconto.
ALTER TABLE subscriptions
    ADD COLUMN IF NOT EXISTS plan_discount_monthly NUMERIC(14, 2) NOT NULL DEFAULT 0;
