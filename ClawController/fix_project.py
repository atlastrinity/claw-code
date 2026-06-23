#!/usr/bin/env python3
import sys
import uuid
import re

if len(sys.argv) < 3:
    print("Usage: python3 fix_project.py <project_path> <package_path>")
    sys.exit(1)

project_path = sys.argv[1]
package_path = sys.argv[2]

with open(project_path, 'r') as f:
    content = f.read()

# Generate UUIDs
uuid1 = str(uuid.uuid4()).lower()
uuid2 = str(uuid.uuid4()).lower()
uuid3 = str(uuid.uuid4()).lower()

# Calculate relative path
import os
project_dir = os.path.dirname(os.path.abspath(project_path))
package_dir = os.path.dirname(os.path.abspath(package_path))
relative_path = os.path.relpath(package_dir, project_dir)

print(f"Project dir: {project_dir}")
print(f"Package dir: {package_dir}")
print(f"Relative path: {relative_path}")

# Fix packageReferences pattern (3 tabs)
old_pattern = "\t\t\tpackageReferences = (\n\t\t\t\t);"
new_pattern = f"\t\t\tpackageReferences = (\n\t\t\t\t{uuid1} /* ClawControllerPackage */;\n\t\t\t);"

content = content.replace(old_pattern, new_pattern)

# Add local package reference section
local_pkg_section = f"""\t/* Begin XCLocalSwiftPackageReferenceSection */
\t\t{uuid2} /* ClawControllerPackage */ = {{
\t\t\tisa = XCLocalSwiftPackageReference;
\t\t\tpath = {relative_path};
\t\t}};
/* End XCLocalSwiftPackageReferenceSection */
"""

# Insert after XCSwiftPackageProductDependency section
content = content.replace(
    "/* Begin XCSwiftPackageProductDependency section */",
    "/* Begin XCLocalSwiftPackageReferenceSection */\n" + local_pkg_section + "\n/* Begin XCSwiftPackageProductDependency section */"
)

# Add package product dependency
product_dep = f"""\t/* Begin XCSwiftPackageProductDependency section */
\t\t{uuid3} /* ClawControllerFeature */ = {{
\t\t\tisa = XCSwiftPackageProductDependency;
\t\t\tpackage = {uuid2} /* ClawControllerPackage */;
\t\t\tproductName = ClawControllerFeature;
\t\t}};
/* End XCSwiftPackageProductDependency section */
"""

# Insert before the closing brace
content = content.replace(
    "/* End XCSwiftPackageProductDependency section */\n\t\t};",
    product_dep + "\n/* End XCSwiftPackageProductDependency section */\n\t\t};"
)

with open(project_path, 'w') as f:
    f.write(content)

print("Successfully updated project file")
