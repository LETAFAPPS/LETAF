-- Foto de perfil do operador (admin/funcionário/super admin).
-- Imagem JPEG/PNG em base64; NULL = sem foto. Editável pelo próprio
-- usuário via PUT /auth/profile (§11 — só a si mesmo).
ALTER TABLE users ADD COLUMN avatar TEXT;
