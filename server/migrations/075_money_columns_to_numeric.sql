-- Converte colunas monetárias de DOUBLE PRECISION para NUMERIC(14, 2)
-- (auditoria #4 — dinheiro exato via rust_decimal::Decimal).
--
-- As migrations 004…069 originalmente criavam essas colunas como
-- DOUBLE PRECISION. Foram editadas em commit posterior para já nascerem
-- NUMERIC(14,2) — o que quebra o checksum do sqlx em qualquer ambiente
-- que já as tivesse aplicado (VersionMismatch). Por isso os arquivos
-- históricos foram revertidos ao conteúdo original e esta migration nova
-- faz a conversão de tipo de forma explícita e idempotente: em banco
-- novo (coluna já DOUBLE PRECISION recém-criada) e em banco existente
-- (coluna DOUBLE PRECISION com dados), o resultado final é o mesmo.
--
-- USING col::numeric(14,2) preserva os valores (arredondando para 2
-- casas), sem perda de precisão relevante para dinheiro em R$.

ALTER TABLE products
    ALTER COLUMN price          TYPE NUMERIC(14, 2) USING price::numeric(14, 2),
    ALTER COLUMN cost_price     TYPE NUMERIC(14, 2) USING cost_price::numeric(14, 2),
    ALTER COLUMN discount_value TYPE NUMERIC(14, 2) USING discount_value::numeric(14, 2);

ALTER TABLE orders
    ALTER COLUMN total             TYPE NUMERIC(14, 2) USING total::numeric(14, 2),
    ALTER COLUMN discount_amount   TYPE NUMERIC(14, 2) USING discount_amount::numeric(14, 2),
    ALTER COLUMN additional_amount TYPE NUMERIC(14, 2) USING additional_amount::numeric(14, 2);

ALTER TABLE order_items
    ALTER COLUMN unit_price TYPE NUMERIC(14, 2) USING unit_price::numeric(14, 2),
    ALTER COLUMN subtotal   TYPE NUMERIC(14, 2) USING subtotal::numeric(14, 2);

ALTER TABLE addons
    ALTER COLUMN price TYPE NUMERIC(14, 2) USING price::numeric(14, 2);

ALTER TABLE coupons
    ALTER COLUMN discount_value  TYPE NUMERIC(14, 2) USING discount_value::numeric(14, 2),
    ALTER COLUMN min_order_value TYPE NUMERIC(14, 2) USING min_order_value::numeric(14, 2),
    ALTER COLUMN max_discount    TYPE NUMERIC(14, 2) USING max_discount::numeric(14, 2);

ALTER TABLE cash_sessions
    ALTER COLUMN initial_change TYPE NUMERIC(14, 2) USING initial_change::numeric(14, 2),
    ALTER COLUMN counted_cash   TYPE NUMERIC(14, 2) USING counted_cash::numeric(14, 2);

ALTER TABLE cash_movements
    ALTER COLUMN amount TYPE NUMERIC(14, 2) USING amount::numeric(14, 2);

ALTER TABLE finance_entries
    ALTER COLUMN amount TYPE NUMERIC(14, 2) USING amount::numeric(14, 2);

ALTER TABLE wallet_accounts
    ALTER COLUMN balance      TYPE NUMERIC(14, 2) USING balance::numeric(14, 2),
    ALTER COLUMN credit_limit TYPE NUMERIC(14, 2) USING credit_limit::numeric(14, 2);

ALTER TABLE wallet_movements
    ALTER COLUMN amount        TYPE NUMERIC(14, 2) USING amount::numeric(14, 2),
    ALTER COLUMN balance_after TYPE NUMERIC(14, 2) USING balance_after::numeric(14, 2);

ALTER TABLE subscription_invoices
    ALTER COLUMN amount TYPE NUMERIC(14, 2) USING amount::numeric(14, 2);

ALTER TABLE payment_charges
    ALTER COLUMN amount TYPE NUMERIC(14, 2) USING amount::numeric(14, 2);

ALTER TABLE plans
    ALTER COLUMN amount TYPE NUMERIC(14, 2) USING amount::numeric(14, 2);

ALTER TABLE subscriptions
    ALTER COLUMN plan_amount            TYPE NUMERIC(14, 2) USING plan_amount::numeric(14, 2),
    ALTER COLUMN plan_discount_monthly  TYPE NUMERIC(14, 2) USING plan_discount_monthly::numeric(14, 2);
