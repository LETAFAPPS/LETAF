-- Trilha de auditoria das ações do super admin (§11).
-- Registro IMUTÁVEL: só INSERT e SELECT (sem update/delete/soft delete).
-- Nível plataforma: sem company_id e sem sync com o desktop.
CREATE TABLE admin_audit_log (
    id UUID PRIMARY KEY,
    actor_id UUID NOT NULL,
    actor_name TEXT NOT NULL,
    action TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id UUID,
    target_label TEXT NOT NULL DEFAULT '',
    details TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Consulta padrão do painel: mais recentes primeiro.
CREATE INDEX idx_admin_audit_created_at ON admin_audit_log (created_at DESC);
