-- Cupom aplicado no pedido (Fase 8 — aplicação no checkout web).
-- `coupon_code` é o registro de uso (contagem de limites via orders).
-- `discount_amount` é calculado no servidor (nunca vindo do frontend).
ALTER TABLE orders ADD COLUMN coupon_code TEXT NULL;
ALTER TABLE orders ADD COLUMN discount_amount NUMERIC(14, 2) NOT NULL DEFAULT 0;

CREATE INDEX orders_company_coupon_idx
    ON orders (company_id, coupon_code)
    WHERE deleted_at IS NULL AND coupon_code IS NOT NULL;
