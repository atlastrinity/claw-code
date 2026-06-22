# Task Roadmap: Claw-Code iOS Remote Client

## 1. Проєктна інфраструктура та моделі даних [in_progress]
- [x] 1.1. Аналіз структури проєкту `ClawController`
- [x] 1.2. Створення базових структур даних (Models) для WebSocket-повідомлень (TaskNode, etc)
- [x] 1.3. Реалізація `Decodable` для отриманих JSON-даних [completed]

## 2. Мережевий рівень (Networking) [pending]
- [x] 2.1. Створення `WebSocketManager` [completed]
- [x] 2.2. Реалізація механізму реконнекту [completed]
- [x] 2.3. Створення потоку даних (ObservableObject) для оновлення UI [completed]

## 3. UI/UX: Чат та Моніторинг [pending]
- [x] 3.1. Створення базового SwiftUI-інтерфейсу чату (ChatView) [completed]
- [x] 3.2. Розробка компоненту для відображення ієрархії завдань (TaskDashboardView) [completed]
- [x] 3.3. Реалізація відображення статусів (pending, in_progress, completed, failed) [completed]

## 4. Фільтрація та логування [pending]
- [x] 4.1. Створення інтерфейсу фільтрації логів [completed]
- [x] 4.2. Інтеграція фільтрів у стрім даних [completed]

## 5. Тестування та Верифікація [pending]
- [x] 5.1. Мокове тестування WebSocket-з'єднання [completed]
- [x] 5.2. Верифікація відображення Task Graph в UI [completed]
