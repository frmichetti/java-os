# Spec Delta

## Purpose

Fornece a um programa C de modo usuário do Java OS as operações básicas de uma libc hospedeira — alocação dinâmica de memória, entrada/saída de arquivo pelas funções padrão da linguagem C, e término de processo — todas implementadas sobre as chamadas de sistema do kernel já existentes.

## ADDED Requirements

### Requirement: Alocação Dinâmica de Memória
Um programa C de modo usuário SHALL poder alocar e liberar memória dinamicamente através das funções padrão de alocação da linguagem C, com cada alocação bem-sucedida resultando numa região de memória utilizável, distinta e do tamanho pedido.

#### Scenario: Memória alocada é utilizável e isolada de outras alocações
- **WHEN** um programa C aloca dinamicamente um bloco de memória de tamanho `N`
- **THEN** o bloco retornado SHALL ter pelo menos `N` bytes utilizáveis, sem sobrepor nenhum outro bloco atualmente alocado

#### Scenario: Múltiplas alocações sucessivas não falham antes de esgotar o espaço disponível
- **WHEN** um programa C faz múltiplas alocações dinâmicas sucessivas, cada uma de tamanho razoável
- **THEN** cada alocação SHALL ter sucesso até que o espaço de heap disponível ao processo seja de fato esgotado

### Requirement: Entrada/Saída de Arquivo pelas Funções Padrão da Linguagem C
Um programa C de modo usuário SHALL poder abrir, ler o conteúdo e fechar um arquivo do sistema de arquivos do Java OS através das funções padrão de E/S de arquivo da linguagem C, obtendo exatamente os mesmos bytes que uma leitura direta do arquivo produziria.

#### Scenario: Leitura de um arquivo via funções padrão de E/S devolve seu conteúdo correto
- **WHEN** um programa C abre e lê um arquivo existente através das funções padrão de E/S de arquivo
- **THEN** os bytes obtidos SHALL corresponder exatamente ao conteúdo real desse arquivo

### Requirement: Escrita no Console pelas Funções Padrão da Linguagem C
Um programa C de modo usuário SHALL poder escrever texto no console de saída do Java OS através das funções padrão de saída da linguagem C (ex.: saída padrão).

#### Scenario: Texto escrito pelas funções padrão de saída aparece no console
- **WHEN** um programa C escreve uma sequência de caracteres através das funções padrão de saída da linguagem C
- **THEN** essa sequência SHALL aparecer no console de saída do kernel

### Requirement: Término de Processo
Um programa C de modo usuário SHALL poder terminar sua própria execução através da função padrão de término de processo da linguagem C (ou ao retornar normalmente de sua função principal), de forma que nenhum código seu volte a ser executado depois disso.

#### Scenario: Chamar a função de término interrompe definitivamente a execução do programa
- **WHEN** um programa C invoca a função padrão de término de processo
- **THEN** nenhuma instrução desse programa SHALL ser executada novamente a partir desse ponto
