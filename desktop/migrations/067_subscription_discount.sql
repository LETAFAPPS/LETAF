-- Desconto comercial (R$/mês) por estabelecimento — espelha o servidor.
-- O billing roda no servidor via `terms()` (que aplica o desconto); aqui
-- é só o espelho local sincronizado. `0` = sem desconto.
ALTER TABLE subscriptions ADD COLUMN plan_discount_monthly REAL NOT NULL DEFAULT 0;
