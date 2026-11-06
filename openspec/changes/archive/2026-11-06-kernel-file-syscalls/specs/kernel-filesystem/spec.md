# Spec Delta

## ADDED Requirements

### Requirement: Abertura de Arquivo por Handle
Dado um sistema de arquivos FAT32 montado, o kernel SHALL permitir abrir um arquivo identificado pelo seu caminho absoluto, devolvendo um identificador (`handle`) opaco com um cursor de leitura posicionado no início do arquivo, distinto do handle de qualquer outro arquivo aberto simultaneamente.

#### Scenario: Abertura de um arquivo existente devolve um handle utilizável
- **WHEN** um caminho absoluto válido, apontando para um arquivo existente, é aberto
- **THEN** o kernel SHALL devolver um `handle` que identifica esse arquivo, com o cursor de leitura posicionado no início do arquivo

#### Scenario: Abertura de um caminho que não corresponde a um arquivo existente é rejeitada
- **WHEN** um caminho absoluto é aberto, mas não corresponde a nenhum arquivo existente (caminho inexistente, ou apontando para um diretório)
- **THEN** o kernel SHALL devolver um erro explícito, sem produzir nenhum handle

#### Scenario: Dois arquivos abertos simultaneamente têm handles distintos e cursores independentes
- **WHEN** dois arquivos (ou o mesmo arquivo duas vezes) são abertos sem que nenhum dos handles resultantes seja fechado entre as duas aberturas
- **THEN** os dois handles devolvidos SHALL ser distintos, e avançar o cursor de um deles através de leitura SHALL NOT afetar a posição do cursor do outro

### Requirement: Leitura Sequencial a Partir do Cursor de um Handle Aberto
Dado um handle de arquivo aberto, o kernel SHALL permitir ler, a partir da posição atual do seu cursor, até uma quantidade de bytes pedida pelo chamador, avançando o cursor exatamente pela quantidade de bytes efetivamente lida.

#### Scenario: Leitura dentro dos limites do arquivo devolve os bytes esperados e avança o cursor
- **WHEN** uma leitura de `N` bytes é pedida num handle cujo cursor tem pelo menos `N` bytes restantes até o fim do arquivo
- **THEN** o kernel SHALL devolver exatamente esses `N` bytes, na ordem correta, e o cursor SHALL avançar `N` bytes

#### Scenario: Leitura que ultrapassa o fim do arquivo devolve só os bytes restantes
- **WHEN** uma leitura é pedida num handle cujo cursor tem menos bytes restantes do que a quantidade pedida
- **THEN** o kernel SHALL devolver exatamente os bytes restantes até o fim do arquivo, e o cursor SHALL avançar até o fim do arquivo

#### Scenario: Leitura com o cursor já no fim do arquivo devolve zero bytes
- **WHEN** uma leitura é pedida num handle cujo cursor já está na posição correspondente ao fim do arquivo
- **THEN** o kernel SHALL devolver zero bytes lidos, sem erro, e o cursor SHALL permanecer no fim do arquivo

#### Scenario: Leitura com um handle desconhecido ou já fechado é rejeitada
- **WHEN** uma leitura é pedida com um `handle` que não corresponde a nenhum arquivo atualmente aberto
- **THEN** o kernel SHALL devolver um erro explícito, sem ler nenhum dado

### Requirement: Fechamento de Handle de Arquivo Aberto
O kernel SHALL permitir fechar um handle de arquivo previamente aberto, liberando os recursos associados a ele e tornando esse handle inválido para operações futuras.

#### Scenario: Fechamento de um handle aberto libera seus recursos
- **WHEN** um handle correspondente a um arquivo atualmente aberto é fechado
- **THEN** o kernel SHALL liberar os recursos associados a esse handle, e uma leitura subsequente com o mesmo handle SHALL ser rejeitada como handle desconhecido

#### Scenario: Fechamento de um handle desconhecido ou já fechado é rejeitado
- **WHEN** um fechamento é pedido com um `handle` que não corresponde a nenhum arquivo atualmente aberto
- **THEN** o kernel SHALL devolver um erro explícito, sem afetar nenhum outro handle aberto
