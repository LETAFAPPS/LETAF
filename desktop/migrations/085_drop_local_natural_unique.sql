-- Remove do SQLite os índices ÚNICOS por chave natural que o servidor já
-- arbitra. Mesma decisão da migration 071 (`orders.number`), pelo mesmo
-- motivo — e agora com um caso reproduzido.
--
-- O `sync_upsert` arbitra por `id`. Quando dois terminais criam offline o
-- MESMO cupom (`code`) ou o MESMO usuário (`email`), cada um nasce com id
-- próprio. Ao puxar o registro do outro terminal, o INSERT viola o índice
-- único local, o erro sobe pelo `?` do `pull_*` e o cursor daquela entidade
-- NÃO avança. Resultado: `coupons`/`users` congelam naquele terminal para
-- sempre — o ciclo seguinte rebaixa o mesmo registro e falha igual, e a
-- reconciliação também. Não havia autocura.
--
-- A unicidade continua valendo onde é autoridade (§11): o Postgres mantém
-- os índices e recusa o segundo registro no push, que fica visível na fila
-- de pendentes. No SQLite o índice não protegia nada que o servidor não
-- protegesse — só impedia o dado de descer.

DROP INDEX IF EXISTS coupons_company_code_uidx;
CREATE INDEX IF NOT EXISTS coupons_company_code_idx
    ON coupons (company_id, code)
    WHERE deleted_at IS NULL;

-- `users.email` é UNIQUE inline no CREATE TABLE (001) — não há índice
-- nomeado para dropar, é preciso recriar a tabela. As colunas abaixo
-- espelham o schema atual (001 + as ALTERs que vieram depois).
CREATE TABLE users_novo (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL REFERENCES companies(id),
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0,
    role TEXT NOT NULL DEFAULT 'admin',
    job_role_id TEXT,
    avatar TEXT,
    phone TEXT
);

INSERT INTO users_novo (id, company_id, email, password_hash, name,
                        created_at, updated_at, deleted_at, synced,
                        role, job_role_id, avatar, phone)
SELECT id, company_id, email, password_hash, name,
       created_at, updated_at, deleted_at, synced,
       role, job_role_id, avatar, phone
  FROM users;

DROP TABLE users;
ALTER TABLE users_novo RENAME TO users;

-- Recria os índices NÃO-únicos que existiam (o DROP TABLE levou todos).
CREATE INDEX IF NOT EXISTS idx_users_company_id ON users(company_id);
CREATE INDEX IF NOT EXISTS idx_users_company_synced ON users(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_users_company_updated ON users(company_id, updated_at);
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_company_email ON users(company_id, email);
