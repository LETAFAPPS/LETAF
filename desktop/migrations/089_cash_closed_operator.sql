-- Nome de quem FECHOU a sessão de caixa (snapshot, como `operator_name` é
-- de quem abriu). NULL nas sessões já fechadas antes desta coluna e nas
-- abertas; a UI mostra o operador de abertura como fallback.
ALTER TABLE cash_sessions ADD COLUMN closed_operator_name TEXT;
