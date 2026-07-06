-- Índices para lookups por e-mail (AI_RULES §13 — desempenho).
-- `customers.find_by_email` filtra (company_id, email); só havia
-- idx_customers_company_id (coluna líder company_id não cobre o e-mail).
CREATE INDEX IF NOT EXISTS idx_customers_company_email ON customers(company_id, email);

-- `users.find_by_email_global` filtra só por email (login desktop resolve
-- o tenant pelo e-mail): o UNIQUE(company_id, email) não serve (líder é
-- company_id). Índice dedicado evita varredura da tabela users.
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
