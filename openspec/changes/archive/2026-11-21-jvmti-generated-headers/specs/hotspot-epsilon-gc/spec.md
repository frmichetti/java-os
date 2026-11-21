# Spec Delta

## Purpose

Confirma e registra `share/gc/epsilon` como o coletor de lixo escolhido para o primeiro porte da JVM sobre o Java OS, com seu código-fonte verificado por compilação real contra a árvore vendorizada do HotSpot.

## ADDED Requirements

### Requirement: Epsilon é o Coletor de Lixo Escolhido para o Primeiro Porte
O projeto SHALL adotar `share/gc/epsilon` (Epsilon GC) como o coletor de
lixo do primeiro porte do HotSpot — a escolha de menor número de
arquivos entre as opções disponíveis na árvore vendorizada, suficiente
para um primeiro porte que nunca precisa de coleta real de lixo.

#### Scenario: Decisão registrada aponta para o diretório real do GC escolhido
- **WHEN** alguém consulta qual coletor de lixo este porte usa
- **THEN** a resposta SHALL apontar para `share/gc/epsilon` na árvore
  vendorizada, como a escolha registrada e verificada

### Requirement: Código-Fonte do Epsilon Compila Contra o HotSpot Vendorizado
Cada arquivo `.cpp` de `share/gc/epsilon` SHALL compilar sem erro via o
mesmo processo de verificação (`g++ -fsyntax-only`) já usado para
verificar outras partes do HotSpot vendorizado neste projeto.

#### Scenario: Todos os arquivos `.cpp` do Epsilon compilam sem erro
- **WHEN** cada um dos arquivos `.cpp` de `share/gc/epsilon` é
  verificado por compilação real contra a árvore vendorizada
- **THEN** nenhum SHALL produzir erro de compilação
