# Spec Delta

## Purpose

Fornece ao kernel acesso de leitura e escrita a um dispositivo de armazenamento em blocos (disco), por índice de bloco (LBA), sem depender de nenhum sistema de arquivos.

## ADDED Requirements

### Requirement: Leitura de um Bloco por Índice Devolve o Conteúdo Real do Disco
O kernel SHALL ser capaz de ler o conteúdo de um bloco do disco de armazenamento, identificado pelo seu índice (LBA), devolvendo exatamente os bytes armazenados nesse bloco.

#### Scenario: Ler um bloco já conhecido devolve seu conteúdo correto
- **WHEN** o kernel lê o bloco de índice conhecido que contém a assinatura de inicialização (os dois últimos bytes do setor de boot, `0x55AA`)
- **THEN** o conteúdo lido SHALL conter essa assinatura exatamente nessa posição, confirmando que os bytes devolvidos são os do disco real, não dados inventados ou de outro bloco

### Requirement: Escrita de um Bloco por Índice Persiste no Disco
O kernel SHALL ser capaz de escrever o conteúdo de um bloco do disco de armazenamento, identificado pelo seu índice (LBA), de forma que uma leitura subsequente desse mesmo índice devolva o conteúdo escrito.

#### Scenario: Ler depois de escrever devolve o conteúdo escrito
- **WHEN** o kernel escreve um conteúdo conhecido em um bloco e, em seguida, lê esse mesmo índice de bloco
- **THEN** o conteúdo lido SHALL ser exatamente o que foi escrito, não o conteúdo anterior do bloco nem o de outro bloco

### Requirement: Tamanho do Bloco é Consistente e Conhecido
O kernel SHALL expor o tamanho (em bytes) de cada bloco do dispositivo de armazenamento, de forma que código consumidor saiba quantos bytes uma leitura ou escrita de um bloco envolve.

#### Scenario: Tamanho de bloco reportado corresponde ao conteúdo lido
- **WHEN** o kernel lê um bloco do disco
- **THEN** a quantidade de bytes devolvida SHALL corresponder exatamente ao tamanho de bloco reportado pelo próprio dispositivo
