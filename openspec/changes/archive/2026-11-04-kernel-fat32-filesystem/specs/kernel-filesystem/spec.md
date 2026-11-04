# Spec Delta

## Purpose

Permite montar um sistema de arquivos FAT32 (somente leitura, nomes curtos 8.3) sobre um dispositivo de armazenamento em blocos já existente, localizando a partição via a tabela de partição GPT, e ler o conteúdo completo de um arquivo dado seu caminho absoluto — a base de `open`/`read` POSIX para carregar classes do class path da JVM.

## ADDED Requirements

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
