-- Fuso da loja como offset fixo de UTC (minutos), para validar janelas de
-- horário (disponibilidade de produto e loja aberta) no backend a partir do
-- agora em UTC. -180 = horário de Brasília (BRT); offset fixo basta no Brasil
-- (sem horário de verão). DEFAULT cobre as linhas existentes e o INSERT de
-- `create` (que não vincula todas as colunas).
ALTER TABLE companies ADD COLUMN IF NOT EXISTS utc_offset_minutes INTEGER NOT NULL DEFAULT -180;
