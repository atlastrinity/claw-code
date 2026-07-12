# Improved Task Planning Process

## 🎯 **Core Principles**

### 1. **Pre-Documentation Rule (CRITICAL)**
Every single micro-action, tool execution, or command MUST be documented as a sub-task in the TaskGraph **BEFORE** you execute it.

**❌ WRONG:**
```swift
// Execute code
xcodebuild build
```

**✅ CORRECT:**
```json
{
  "id": "1.2.1.1",
  "parent_id": "1.2",
  "content": "Execute xcodebuild to compile project",
  "status": "in_progress"
}
```

---

### 2. **Deep Recursion Template**
Always break tasks down into **6-7 levels of depth** for granular micro-actions.

#### **Template for Recursive Breakdown:**

```
Main Task (Level 0)
├── Phase 1: Preparation (Level 1)
│   ├── Sub-task 1.1 (Level 2)
│   │   ├── Micro-action 1.1.1 (Level 3)
│   │   │   ├── Tiny-step 1.1.1.1 (Level 4)
│   │   │   │   └── Atom 1.1.1.1.1 (Level 5)
│   │   │   └── Tiny-step 1.1.1.2 (Level 4)
│   │   └── Micro-action 1.1.2 (Level 3)
│   └── Sub-task 1.2 (Level 2)
├── Phase 2: Execution (Level 1)
│   └── ...
└── Phase 3: Verification (Level 1)
    └── ...
```

---

### 3. **Validation Criteria (Before Execution)**

#### **Checklist for Every Task Addition:**

**✅ Required Fields:**
- [ ] `id`: Unique identifier (string format like "1.2.1.1")
- [ ] `parent_id`: Must exist in the tree
- [ ] `content`: Clear, actionable description
- [ ] `status`: Must be "pending" or "in_progress"

**✅ Recursion Depth Check:**
- [ ] Are we at least 3 levels deep?
- [ ] Can this be broken down further?
- [ ] Is each micro-action verifiable?

**✅ Dependency Check:**
- [ ] Are all parent tasks completed?
- [ ] Is the task logically dependent on parents?
- [ ] Are there circular dependencies?

---

### 4. **Task Completion Verification**

#### **Before Marking as "completed":**

**✅ Execution Verification:**
- [ ] Code was actually created/modified
- [ ] Command executed successfully (check return code)
- [ ] Tool returned expected output
- [ ] No errors or warnings in logs

**✅ Code Verification (if applicable):**
- [ ] Read the generated code using `read_file`
- [ ] Compile/test the code using `xcodebuild` or `cargo test`
- [ ] Verify syntax and structure
- [ ] Check for unintended side effects

**✅ Evidence Required:**
- [ ] Screenshot or log output
- [ ] File creation/modification confirmation
- [ ] Test results or build status
- [ ] Error messages (if any)

---

### 5. **Failure Handling Protocol**

#### **What to Do When a Task Fails:**

**Step 1: Document Failure**
```json
{
  "id": "1.2.1.1",
  "parent_id": "1.2",
  "content": "Execute xcodebuild to compile project",
  "status": "failed"
}
```

**Step 2: Create Alternative Approach**
```json
{
  "id": "1.2.1.2",
  "parent_id": "1.2",
  "content": "Alternative: Clean build and retry xcodebuild",
  "status": "pending"
}
```

**Step 3: Do NOT Delete Failed Tasks**
- Keep failed tasks visible (shown with minus sign)
- They serve as documentation of what didn't work
- Help avoid repeating the same mistakes

---

### 6. **Automated Task Tracking Checks**

#### **Run These Checks After Every Major Phase:**

**Check 1: Task Completeness**
```bash
# Count pending tasks
grep -c "\[ \]" task.md

# Count completed tasks
grep -c "\[x\]" task.md

# Count failed tasks
grep -c "\[-\]" task.md
```

**Check 2: Tree Integrity**
- Verify all parent_ids exist in the tree
- Check for orphaned tasks (no parent)
- Ensure no circular dependencies

**Check 3: Progress Tracking**
- Calculate completion percentage
- Identify blocked tasks (pending with no parent progress)
- Flag tasks that have been pending too long

---

### 7. **Best Practices for Task Naming**

#### **Naming Convention:**
- Use action verbs: "Create", "Update", "Test", "Verify"
- Be specific: "Write SwiftUI view" not "Do work"
- Include scope: "Fix authentication bug in login screen"

#### **Examples:**
- ✅ **Good:** "Create Firebase authentication flow with email/password"
- ❌ **Bad:** "Do Firebase stuff"
- ✅ **Good:** "Add Firestore collection 'users' with fields: id, email, createdAt"
- ❌ **Bad:** "Add database"

---

### 8. **Task Creation Workflow**

#### **Standard Workflow:**

**1. Analyze the Request**
```
What needs to be done?
What dependencies exist?
What is the smallest verifiable step?
```

**2. Design the Hierarchy**
```
Level 0: Main objective
Level 1: High-level phases
Level 2: Major tasks
Level 3: Sub-tasks
Level 4: Micro-actions
Level 5: Tiny-steps
Level 6: Atoms
```

**3. Validate Before Adding**
```
✅ Does this task add value?
✅ Can it be verified?
✅ Is it actionable?
✅ Is it correctly positioned in hierarchy?
```

**4. Add to TaskGraph**
```json
{
  "operation": "add",
  "nodes": [
    {
      "id": "1.2.1.1",
      "parent_id": "1.2",
      "content": "Clear Xcode derived data",
      "status": "pending"
    }
  ]
}
```

**5. Execute Immediately**
- Mark as "in_progress"
- Execute the task
- Mark as "completed" (with verification)

---

### 9. **Common Pitfalls to Avoid**

#### **❌ DON'T:**
- Skip task documentation
- Add tasks without checking parent completion
- Use vague descriptions
- Mark tasks complete without verification
- Delete failed tasks
- Create circular dependencies
- Break tasks into too few levels
- Forget to update task status

#### **✅ DO:**
- Document everything before execution
- Verify each task completion
- Keep failed tasks visible
- Use specific, actionable descriptions
- Maintain proper hierarchy
- Verify code with compilation/testing
- Use TaskGraph tool exclusively

---

### 10. **Example: Complete Task Planning Workflow**

#### **User Request:**
"Create a SwiftUI app with Firebase authentication"

#### **Step 1: Analysis**
- Main task: Create Firebase-authenticated SwiftUI app
- Dependencies: Firebase setup, Xcode project, SwiftUI code
- Smallest step: Create Xcode project structure

#### **Step 2: Design Hierarchy**
```
1. Create Firebase-authenticated SwiftUI app
├── 1.1. Create Xcode project structure
│   ├── 1.1.1. Create project.yml configuration
│   ├── 1.1.2. Initialize Swift Package Manager
│   └── 1.1.3. Configure iOS deployment target
├── 1.2. Setup Firebase dependencies
│   ├── 1.2.1. Add Firebase iOS SDK package
│   ├── 1.2.2. Create GoogleService-Info.plist
│   └── 1.2.3. Configure Firebase in project.yml
├── 1.3. Create SwiftUI authentication view
│   ├── 1.3.1. Create LoginView struct
│   ├── 1.3.2. Implement email/password fields
│   └── 1.3.3. Add Firebase Auth integration
└── 1.4. Test authentication flow
    ├── 1.4.1. Build project in Xcode
    ├── 1.4.2. Run in iOS Simulator
    └── 1.4.3. Verify login functionality
```

#### **Step 3: Validate**
```
✅ All fields present
✅ Hierarchy depth: 4 levels
✅ Tasks are actionable
✅ Dependencies identified
✅ Verification steps included
```

#### **Step 4: Execute**
```json
{
  "operation": "add",
  "nodes": [
    {"id": "1", "content": "Create Firebase-authenticated SwiftUI app", "status": "pending"},
    {"id": "1.1", "content": "Create Xcode project structure", "parent_id": "1", "status": "pending"},
    {"id": "1.1.1", "content": "Create project.yml configuration", "parent_id": "1.1", "status": "pending"},
    {"id": "1.1.2", "content": "Initialize Swift Package Manager", "parent_id": "1.1", "status": "pending"},
    {"id": "1.1.3", "content": "Configure iOS deployment target", "parent_id": "1.1", "status": "pending"}
  ]
}
```

#### **Step 5: Verify Completion**
- Read project.yml (confirmed created)
- Run xcodebuild (confirmed success)
- Check Firebase setup (confirmed configured)

---

## 🚀 **Summary**

The improved task planning process ensures:
- ✅ **No missed tasks** (pre-documentation rule)
- ✅ **No skipped steps** (deep recursion)
- ✅ **No unverified completions** (strict verification)
- ✅ **Clear hierarchy** (proper parent-child relationships)
- ✅ **Better error tracking** (failed tasks remain visible)
- ✅ **Easier debugging** (granular micro-actions)
- ✅ **Improved accountability** (every action documented)

**Always remember:** The TaskGraph is your single source of truth. Never edit task.md directly. Use the TaskGraph tool exclusively.
