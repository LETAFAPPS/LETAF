-- Múltiplas faixas de desconto bulk (Acima de N1 → V1, Acima de N2 → V2, ...).
--
-- Por que JSON ao invés de tabela N:1: o produto sempre é carregado inteiro
-- (catálogo web/PDV não consulta tier isoladamente), então uma tabela extra
-- só adicionaria join e custo de sync sem ganho. Mesmo padrão usado em
-- `availability_schedule`.
--
-- Quando `discount_kind` é `bulk_*` e há `discount_tiers`, ele é a fonte
-- única dos tiers; `discount_value` / `discount_min_qty` ficam NULL.
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS discount_tiers TEXT;

-- Migra o tier único legado para o array. Após mover, zera os antigos
-- para evitar dupla contabilização.
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
