-- Múltiplas faixas de desconto bulk para o desktop (SQLite).
-- JSON: [{"min_qty": f64, "value": f64}, ...]. Quando preenchido, é a
-- fonte única dos tiers; `discount_value`/`discount_min_qty` ficam NULL.
ALTER TABLE products ADD COLUMN discount_tiers TEXT;

UPDATE products
   SET discount_tiers = '[{"min_qty":' || discount_min_qty
                     || ',"value":' || discount_value || '}]'
 WHERE discount_kind LIKE 'bulk_%'
   AND discount_min_qty IS NOT NULL
   AND discount_value IS NOT NULL
   AND discount_tiers IS NULL;

UPDATE products
   SET discount_value = NULL,
       discount_min_qty = NULL
 WHERE discount_kind LIKE 'bulk_%'
   AND discount_tiers IS NOT NULL;
