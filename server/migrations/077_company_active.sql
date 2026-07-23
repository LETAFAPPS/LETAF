-- Bloqueio de acesso do tenant (controle da plataforma / super admin).
-- TRUE = ativa (login liberado); FALSE = suspensa (login recusado).
ALTER TABLE companies ADD COLUMN active BOOLEAN NOT NULL DEFAULT TRUE;
