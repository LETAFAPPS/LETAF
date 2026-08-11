-- Revogação de sessão do cliente final (§11): até aqui o JWT do cliente não
-- tinha versão de credencial (tv fixo 0) e nada o invalidava antes do exp (72h)
-- — trocar a senha ou banir o cliente não derrubava sessões ativas. Esta coluna
-- é AUTORIDADE DO SERVIDOR: NÃO viaja no struct Customer nem no sync (o upsert
-- de sync lista colunas explícitas e não a inclui), então um push do desktop
-- não pode "ressuscitar" uma sessão revogada. Só o servidor a lê/incrementa.
ALTER TABLE customers
    ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;
