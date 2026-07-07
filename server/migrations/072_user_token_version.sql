-- Versão de credencial do usuário (RBAC §11): incrementada quando role,
-- permissões ou senha mudam. O JWT carrega o `tv` emitido; o servidor compara
-- com este valor a cada requisição de operador e rejeita tokens defasados —
-- revogação imediata sem esperar o `exp`. Default 0 casa com o default do
-- claim `tv` (tokens legados continuam válidos até expirar; sem logout em massa).
ALTER TABLE users ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;
