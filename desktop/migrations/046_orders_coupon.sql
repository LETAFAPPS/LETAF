-- Espelha server/048_orders_coupon.sql (Fase 8).
ALTER TABLE orders ADD COLUMN coupon_code TEXT NULL;
ALTER TABLE orders ADD COLUMN discount_amount REAL NOT NULL DEFAULT 0;

CREATE INDEX orders_company_coupon_idx
    ON orders (company_id, coupon_code)
    WHERE deleted_at IS NULL AND coupon_code IS NOT NULL;
