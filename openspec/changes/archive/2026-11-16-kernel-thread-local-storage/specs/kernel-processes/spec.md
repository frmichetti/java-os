# Spec Delta

## ADDED Requirements

### Requirement: Bloco de Armazenamento Local de Thread (TLS) por Thread
Toda thread de modo usuário SHALL ter seu próprio bloco de armazenamento
local de thread (TLS), independente do de qualquer outra thread do mesmo
processo, inicializado a partir da imagem de TLS declarada pelo executável
desse processo quando ele declarar uma — caso contrário, nenhum bloco é
necessário.

#### Scenario: Duas threads têm blocos de TLS independentes
- **WHEN** duas threads distintas de modo usuário (da mesma thread
  principal de um processo, ou de um processo e uma thread adicional
  criada dentro dele) leem e escrevem em seu próprio armazenamento local de
  thread
- **THEN** alterar o conteúdo do bloco de TLS de uma dessas threads SHALL
  NOT afetar o conteúdo do bloco de TLS de nenhuma outra

#### Scenario: Bloco de TLS é inicializado a partir da imagem declarada pelo executável
- **WHEN** uma thread de modo usuário é criada para um processo cujo
  executável declara uma imagem de TLS com conteúdo inicial menor que seu
  tamanho total
- **THEN** o bloco de TLS dessa thread SHALL começar com esse conteúdo
  inicial copiado, e o restante, até o tamanho total declarado, SHALL
  começar zerado

#### Scenario: Executável sem imagem de TLS não exige bloco algum
- **WHEN** uma thread de modo usuário é criada para um processo cujo
  executável não declara nenhuma imagem de TLS
- **THEN** nenhum bloco de TLS SHALL ser necessário para essa thread, sem
  nenhuma mudança de comportamento observável em relação a antes desta
  capacidade existir

#### Scenario: Thread adicional criada num processo existente também tem seu próprio bloco
- **WHEN** uma nova thread de usuário é criada dentro de um processo de
  usuário já existente, cujo executável declara uma imagem de TLS
- **THEN** essa thread adicional SHALL ter seu próprio bloco de TLS,
  inicializado a partir da mesma imagem declarada pelo executável desse
  processo, independente do bloco de TLS de qualquer outra thread já
  existente nele
