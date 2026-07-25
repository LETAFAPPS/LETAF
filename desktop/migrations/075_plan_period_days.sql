-- Período do plano em DIAS (snapshot na assinatura). Converte meses × 30.
ALTER TABLE subscriptions RENAME COLUMN plan_period_months TO plan_period_days;
UPDATE subscriptions SET plan_period_days = plan_period_days * 30;
