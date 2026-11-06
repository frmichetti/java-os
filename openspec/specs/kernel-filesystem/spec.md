# kernel-filesystem Specification

## Purpose

Permite montar um sistema de arquivos FAT32 (somente leitura, nomes curtos 8.3) sobre um dispositivo de armazenamento em blocos já existente, localizando a partição via a tabela de partição GPT, e ler o conteúdo completo de um arquivo dado seu caminho absoluto — a base de `open`/`read` POSIX para carregar classes do class path da JVM.

## Requirements

### Requirement: Montagem do Sistema de Arquivos via Tabela de Partição GPT
O kernel SHALL localizar a partição formatada em FAT32 no disco de armazenamento lendo sua tabela de partição GPT, e montar essa partição como um sistema de arquivos, validando que seu setor de inicialização corresponde de fato a um sistema de arquivos FAT32.

#### Scenario: Montagem bem-sucedida de uma partição FAT32 válida
- **WHEN** o disco de armazenamento contém uma tabela de partição GPT com uma partição do tipo "EFI System Partition" formatada em FAT32
- **THEN** o kernel SHALL montar essa partição com sucesso, tornando suas operações de leitura de arquivo disponíveis

#### Scenario: Nenhuma partição FAT32/EFI é encontrada
- **WHEN** a tabela de partição GPT do disco não contém nenhuma partição do tipo "EFI System Partition"
- **THEN** o kernel SHALL rejeitar a montagem com um erro explícito, sem tratar nenhuma outra partição como se fosse o sistema de arquivos

#### Scenario: Partição encontrada não é um FAT32 válido
- **WHEN** a partição localizada existe, mas seu setor de inicialização não corresponde a um sistema de arquivos FAT32 válido (assinatura de inicialização ausente, ou o tipo de sistema de arquivos declarado não é FAT32)
- **THEN** o kernel SHALL rejeitar a montagem com um erro explícito, em vez de interpretar dados arbitrários como uma estrutura FAT32

### Requirement: Leitura de Arquivo por Caminho Absoluto
Dado um sistema de arquivos FAT32 montado, o kernel SHALL permitir ler o conteúdo completo de um arquivo identificado pelo seu caminho absoluto (a partir da raiz do sistema de arquivos), devolvendo exatamente os bytes armazenados nesse arquivo, na ordem correta, independentemente de quantos clusters ele ocupe.

#### Scenario: Leitura de um arquivo existente devolve seu conteúdo completo e correto
- **WHEN** um caminho absoluto válido, apontando para um arquivo existente, é lido
- **THEN** o kernel SHALL devolver exatamente os bytes desse arquivo, na quantidade igual ao seu tamanho declarado, na ordem correta — mesmo que o arquivo ocupe múltiplos clusters não necessariamente contíguos no disco

#### Scenario: Caminho não corresponde a nenhum arquivo ou diretório existente
- **WHEN** um caminho absoluto é lido, mas algum componente do caminho (intermediário ou final) não corresponde a nenhuma entrada existente no diretório correspondente
- **THEN** o kernel SHALL devolver um erro explícito, em vez de ler dados de outro arquivo ou de memória não inicializada

#### Scenario: Caminho aponta para um diretório, não um arquivo
- **WHEN** um caminho absoluto é lido, mas corresponde a um diretório em vez de um arquivo
- **THEN** o kernel SHALL devolver um erro explícito, em vez de tentar ler o diretório como se fosse um arquivo

#### Scenario: Um componente intermediário do caminho não é um diretório
- **WHEN** um caminho absoluto é lido, mas um componente intermediário (não o último) corresponde a um arquivo em vez de um diretório
- **THEN** o kernel SHALL devolver um erro explícito, em vez de tentar continuar a resolução do caminho através dele

### Requirement: Correspondência de Nomes Curtos Sem Distinção de Maiúsculas/Minúsculas
A resolução de cada componente de um caminho contra as entradas de um diretório SHALL ser insensível a maiúsculas/minúsculas, consistente com a convenção de nomes curtos 8.3 do FAT32.

#### Scenario: Caminho em minúsculas encontra uma entrada armazenada em maiúsculas
- **WHEN** um caminho absoluto é fornecido com letras minúsculas, e a entrada correspondente no disco está armazenada em maiúsculas (convenção usual de nomes curtos 8.3)
- **THEN** o kernel SHALL encontrar e ler essa entrada normalmente, em vez de tratá-la como inexistente

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
