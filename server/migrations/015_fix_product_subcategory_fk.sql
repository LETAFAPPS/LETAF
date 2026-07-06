-- Corrige FK incorreta introduzida na migration 011.
-- subcategory_id foi criado com REFERENCES categories(id) por engano.
-- O sync do desktop falha ao enviar produtos com subcategoria porque
-- o UUID de subcategoria nao existe na tabela categories.
-- Remove a constraint errada; a integridade e garantida no service.
ALTER TABLE products DROP CONSTRAINT IF EXISTS products_subcategory_id_fkey;
