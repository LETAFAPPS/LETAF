-- Período do plano passa de MESES para DIAS (ponta a ponta).
-- Converte os valores existentes (meses × 30) e renomeia as colunas.
ALTER TABLE plans RENAME COLUMN period_months TO period_days;
UPDATE plans SET period_days = period_days * 30;

ALTER TABLE subscriptions RENAME COLUMN plan_period_months TO plan_period_days;
UPDATE subscriptions SET plan_period_days = plan_period_days * 30;
