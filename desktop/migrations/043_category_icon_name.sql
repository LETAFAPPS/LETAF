-- Espelha server/045_category_icon_name.sql (Fase 6).
-- Sync de Category passa a transportar `icon_name` (Option<String>).

ALTER TABLE categories ADD COLUMN icon_name TEXT NULL;
