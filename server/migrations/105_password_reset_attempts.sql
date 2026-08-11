-- Anti-força-bruta do código de reset (§11): o código é de 6 dígitos (10^6
-- combinações) com TTL de 15 min. Até aqui só havia throttle por IP, então uma
-- botnet (muitos IPs) podia adivinhar o código dentro do TTL. Este contador de
-- tentativas por CÓDIGO permite invalidá-lo após N falhas — defesa que não
-- depende do número de IPs do atacante.
ALTER TABLE password_resets
    ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0;
