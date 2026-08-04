-- Espelho da 089 do SQLite: nome de quem fechou a sessão de caixa. Precisa
-- existir no servidor para viajar no sync das sessões (§7).
ALTER TABLE cash_sessions ADD COLUMN IF NOT EXISTS closed_operator_name TEXT;
