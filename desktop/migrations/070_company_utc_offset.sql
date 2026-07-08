-- Espelha server/migrations/074: fuso da loja como offset fixo de UTC (min).
-- -180 = BRT. DEFAULT cobre linhas existentes e o INSERT de `create`.
ALTER TABLE companies ADD COLUMN utc_offset_minutes INTEGER NOT NULL DEFAULT -180;
