module('Tree', {
  
  setup: function() {
    this.branch = new Tree.Branch([
      new Tree.Node(),
      new Tree.Node(),
      new Tree.Node()
    ]);
    
    this.node = new Tree.Node(this.branch);
    
    this.tree = new Tree.Root(
      new Tree.Branch([
        new Tree.Node(new Tree.Branch([
          new Tree.Node(),
          new Tree.Node()
        ]), {}),
        new Tree.Node(new Tree.Branch([
          new Tree.Node()
        ]), {}),
        new Tree.Node(new Tree.Branch([
          new Tree.Node(),
          new Tree.Node(),
          this.node,
          new Tree.Node()
        ]), {})
      ])
    );
  },
  
  teardown: function() {
    delete this.branch;
    delete this.node;
    delete this.tree;
  }
  
});

test('find node', function() {
  var node = new Tree.Node();
  
  equals(this.tree.find(this.node.id), this.node);
  equals(this.tree.find(node.id), null);
});

test('insert node', function() {
  var node = new Tree.Node();
  this.tree.insert(node, this.node.id);
  
  equals(this.node.children.size(), 4);
  equals(_.last(this.node.children.nodes), node);
  equals(node.branch, this.branch);
  equals(this.tree.find(node.id), node);
});

test('insert node at index', function() {
  var node = new Tree.Node();
  this.tree.insert(node, this.node.id, 1);
  
  equals(this.node.children.size(), 4);
  equals(this.node.children.nodes[1], node);
  equals(node.branch, this.branch);
  equals(this.tree.find(node.id), node);
});

test('set index', function() {
  this.tree.setIndex(this.node.id, 1);
  
  equals(this.node.branch.size(), 4);
  equals(this.node.branch.nodes[1], this.node);
  equals(this.tree.find(this.node.id), this.node);
});