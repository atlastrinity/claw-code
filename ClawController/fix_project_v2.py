#!/usr/bin/env python3
import re

with open('ClawController.xcodeproj/project.pbxproj', 'r') as f:
    content = f.read()

# Find the packageReferences line with 3 tabs
# Pattern: \t\t\tpackageReferences = (\n\t\t\t\t);
old_pattern = "\t\t\tpackageReferences = (\n\t\t\t\t);"

# Generate a real UUID
import uuid
pkg_uuid = str(uuid.uuid4()).lower()
pkg2_uuid = str(uuid.uuid4()).lower()
pkg3_uuid = str(uuid.uuid4()).lower()

# Replace the empty packageReferences
new_pattern = f"\t\t\tpackageReferences = (\n\t\t\t\t{pkg_uuid} /* ClawControllerPackage */;\n\t\t\t);"

content = content.replace(old_pattern, new_pattern)

# Add local package reference section
local_pkg_ref = f"""\t/* Begin XCLocalSwiftPackageReferenceSection */
\t\t{pkg2_uuid} /* ClawControllerPackage */ = {{
\t\t\tisa = XCLocalSwiftPackageReference;
\t\t\tpath = ..;
\t\t}};
/* End XCLocalSwiftPackageReferenceSection */
"""

# Insert before XCSwiftPackageProductDependency section
content = content.replace(
    "/* Begin XCSwiftPackageProductDependency section */",
    "/* Begin XCLocalSwiftPackageReferenceSection */\n" + local_pkg_ref + "\n/* Begin XCSwiftPackageProductDependency section */"
)

# Add package product dependency
product_dep = f"""\t/* Begin XCSwiftPackageProductDependency section */
\t\t{pkg3_uuid} /* ClawControllerFeature */ = {{
\t\t\tisa = XCSwiftPackageProductDependency;
\t\t\tpackage = {pkg2_uuid} /* ClawControllerPackage */;
\t\t\tproductName = ClawControllerFeature;
\t\t}};
/* End XCSwiftPackageProductDependency section */
"""

# Insert before the closing brace of PBXProject
content = content.replace(
    "/* End XCSwiftPackageProductDependency section */\n\t\t};",
    product_dep + "\n/* End XCSwiftPackageProductDependency section */\n\t\t};"
)

with open('ClawController.xcodeproj/project.pbxproj', 'w') as f:
    f.write(content)

print("Successfully updated project file")
print(f"Package UUID: {pkg_uuid}")
print(f"Package2 UUID: {pkg2_uuid}")
print(f"Package3 UUID: {pkg3_uuid}")
